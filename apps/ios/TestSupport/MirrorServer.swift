import Foundation
import Network

/// A local Starter library mirror for the tests (S24): a small HTTP/1.1 server on
/// `127.0.0.1` serving the files of a folder (`dist/starter`), so no test touches the
/// network. The app's transport and background downloads fetch from it as from GitHub's
/// release assets. It answers GET and HEAD, byte ranges (`Range: bytes=N-`, with an ETag and
/// `If-Range`, so a transfer can resume), and for the tests: a throttle, files served
/// corrupted (a bad sha256), files held back until released, and a GitHub-like redirect
/// (`/r/<name>` → 302 to `/signed/<name>?expires=<t>`, refused with 403 once `t` has passed).
/// Every request it got is kept in `requests`.
final class MirrorServer: @unchecked Sendable {
    struct Request: Equatable {
        let method: String
        let path: String
        let range: UInt64?
    }

    let root: URL
    private let listener: NWListener
    private let queue = DispatchQueue(label: "mirror.server")
    private let lock = NSLock()
    private var _requests: [Request] = []
    private var _corrupt: Set<String> = []
    private var _held: Set<String> = []
    private var _bytesPerSecond: Int?
    private var _signedLifetime: TimeInterval = 600
    private(set) var port: UInt16 = 0

    init(root: URL) throws {
        self.root = root
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        parameters.allowLocalEndpointReuse = true
        listener = try NWListener(using: parameters)
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in
            switch state {
            case .ready, .failed, .cancelled: ready.signal()
            default: break
            }
        }
        listener.newConnectionHandler = { [weak self] connection in self?.serve(connection) }
        listener.start(queue: queue)
        _ = ready.wait(timeout: .now() + 10)
        guard let port = listener.port?.rawValue else {
            throw URLError(.cannotConnectToHost)
        }
        self.port = port
    }

    deinit { listener.cancel() }

    func stop() { listener.cancel() }

    /// `http://127.0.0.1:<port>/`.
    var base: String { "http://127.0.0.1:\(port)/" }

    var requests: [Request] { locked { _requests } }

    /// Serve these file names with their bytes changed (a sha256 mismatch).
    func corrupt(_ names: Set<String>) { locked { _corrupt = names } }

    /// Hold these file names back (their GET waits) until `release`.
    func hold(_ names: Set<String>) { locked { _held = names } }

    func release() { locked { _held = [] } }

    /// Send at most this many bytes a second on each connection (nil: as fast as it goes).
    func throttle(_ bytesPerSecond: Int?) { locked { _bytesPerSecond = bytesPerSecond } }

    /// How long a `/signed/` URL from a redirect stays valid.
    func signedLifetime(_ seconds: TimeInterval) { locked { _signedLifetime = seconds } }

    // MARK: Serving

    private func serve(_ connection: NWConnection) {
        connection.start(queue: queue)
        readHead(connection, Data())
    }

    private func readHead(_ connection: NWConnection, _ buffer: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, done, error in
            guard let self else { return }
            var buffer = buffer
            if let data { buffer.append(data) }
            if let end = buffer.range(of: Data("\r\n\r\n".utf8)) {
                let head = String(decoding: buffer[..<end.lowerBound], as: UTF8.self)
                self.respond(connection, head)
            } else if done || error != nil {
                connection.cancel()
            } else {
                self.readHead(connection, buffer)
            }
        }
    }

    private func respond(_ connection: NWConnection, _ head: String) {
        let lines = head.components(separatedBy: "\r\n")
        let parts = (lines.first ?? "").split(separator: " ")
        guard parts.count >= 2 else { return close(connection, status: 400) }
        let method = String(parts[0])
        let target = String(parts[1])
        var headers: [String: String] = [:]
        for line in lines.dropFirst() {
            guard let colon = line.firstIndex(of: ":") else { continue }
            headers[line[..<colon].lowercased()] = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
        }
        var range: UInt64?
        if let r = headers["range"], r.hasPrefix("bytes="), r.hasSuffix("-") {
            range = UInt64(r.dropFirst(6).dropLast())
        }
        let path = target.components(separatedBy: "?").first ?? target
        locked { _requests.append(Request(method: method, path: path, range: range)) }
        let query = target.components(separatedBy: "?").dropFirst().first ?? ""

        if path.hasPrefix("/status/") {
            // `/status/403-limited` is GitHub's rate limit answer; `/status/N` is status N.
            let code = path.dropFirst("/status/".count)
            let limited = code.hasSuffix("-limited")
            let status = Int(code.replacingOccurrences(of: "-limited", with: "")) ?? 500
            return close(connection, status: status, extra: limited ? ["x-ratelimit-remaining: 0"] : [])
        }
        var name = String(path.dropFirst())
        if name.hasPrefix("r/") {
            let file = String(name.dropFirst(2))
            let expires = Date().timeIntervalSince1970 + locked { _signedLifetime }
            return close(connection, status: 302, extra: ["Location: \(base)signed/\(file)?expires=\(Int(expires))"])
        }
        if name.hasPrefix("signed/") {
            let expires = query.components(separatedBy: "=").last.flatMap(Double.init) ?? 0
            if Date().timeIntervalSince1970 > expires { return close(connection, status: 403) }
            name = String(name.dropFirst("signed/".count))
        }
        let file = root.appendingPathComponent(name)
        guard !name.contains(".."), let attributes = try? FileManager.default.attributesOfItem(atPath: file.path),
            (attributes[.type] as? FileAttributeType) == .typeRegular
        else { return close(connection, status: 404) }
        let size = (attributes[.size] as? NSNumber)?.uint64Value ?? 0
        let modified = (attributes[.modificationDate] as? Date) ?? Date(timeIntervalSince1970: 0)
        let etag = "\"\(size)-\(Int(modified.timeIntervalSince1970))\""
        var start: UInt64 = 0
        if let range, range < size, headers["if-range"].map({ $0 == etag }) ?? true {
            start = range
        }
        var head = start > 0 ? "HTTP/1.1 206 Partial Content\r\n" : "HTTP/1.1 200 OK\r\n"
        head += "Content-Length: \(size - start)\r\nAccept-Ranges: bytes\r\nETag: \(etag)\r\n"
        if start > 0 { head += "Content-Range: bytes \(start)-\(size - 1)/\(size)\r\n" }
        head += "Content-Type: application/octet-stream\r\nConnection: close\r\n\r\n"
        let (served, from) = (name, start)
        connection.send(content: Data(head.utf8), completion: .contentProcessed { [weak self] _ in
            guard method == "GET", let self else { return connection.cancel() }
            guard let handle = try? FileHandle(forReadingFrom: file) else { return connection.cancel() }
            try? handle.seek(toOffset: from)
            self.waitIfHeld(served) {
                self.send(connection, handle, name: served, offset: from)
            }
        })
    }

    private func waitIfHeld(_ name: String, then body: @escaping () -> Void) {
        if locked({ _held.contains(name) }) {
            queue.asyncAfter(deadline: .now() + 0.1) { [weak self] in self?.waitIfHeld(name, then: body) }
        } else {
            body()
        }
    }

    private func send(_ connection: NWConnection, _ handle: FileHandle, name: String, offset: UInt64) {
        let chunkSize = 64 * 1024
        guard let read = try? handle.read(upToCount: chunkSize), !read.isEmpty else {
            try? handle.close()
            return connection.send(content: nil, isComplete: true, completion: .contentProcessed { _ in connection.cancel() })
        }
        let (corrupt, rate) = locked { (_corrupt.contains(name), _bytesPerSecond) }
        let chunk = corrupt ? Data(read.map { $0 ^ 0x5A }) : read
        connection.send(content: chunk, completion: .contentProcessed { [weak self] error in
            guard let self, error == nil, connection.state == .ready else {
                try? handle.close()
                return connection.cancel()
            }
            let next = offset + UInt64(chunk.count)
            let delay = rate.map { Double(chunk.count) / Double($0) } ?? 0
            self.queue.asyncAfter(deadline: .now() + delay) {
                self.send(connection, handle, name: name, offset: next)
            }
        })
    }

    private func close(_ connection: NWConnection, status: Int, extra: [String] = []) {
        var head = "HTTP/1.1 \(status) Status\r\nContent-Length: 0\r\nConnection: close\r\n"
        for line in extra { head += line + "\r\n" }
        head += "\r\n"
        connection.send(content: Data(head.utf8), isComplete: true, completion: .contentProcessed { _ in
            connection.cancel()
        })
    }

    private func locked<T>(_ body: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return body()
    }
}
