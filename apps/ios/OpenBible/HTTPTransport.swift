import Foundation
import OpenBibleCore

/// What an HTTP status means, as `apps/openbible/src/fetch.rs` (`check_status`) tells them
/// apart: 2xx is a success; 404 and 410 not found; 429, and GitHub's 403 with no requests
/// remaining, rate limited; anything else a network error naming the URL and the status.
/// The Starter manifest's transport and the background downloads both read statuses here.
enum HTTPStatus {
    static func check(url: String, response: URLResponse?) -> TransportError? {
        guard let http = response as? HTTPURLResponse else { return nil }
        let code = http.statusCode
        let remaining = http.value(forHTTPHeaderField: "x-ratelimit-remaining")
        switch code {
        case 200...299: return nil
        case 404, 410: return .NotFound
        case 429: return .RateLimited
        case 403 where remaining == "0": return .RateLimited
        default: return .Network(message: "\(url): http status: \(code)")
        }
    }

    /// A transfer's error in the words `fetch.rs` uses: the URL, then what went wrong.
    static func failure(url: String, _ error: Error) -> TransportError {
        .Network(message: "\(url): \(error.localizedDescription)")
    }

    /// A `TransportError` as the core's download states take it.
    static func message(_ error: TransportError) -> String {
        switch error {
        case .NotFound: return "not found"
        case .RateLimited: return "rate limited"
        case .Network(let message): return message
        }
    }
}

/// The shell's transport for the small files the core fetches whole (the Starter manifest,
/// GitHub's releases list; `docs/dev/mobile-library.md`): one plain GET, as
/// `apps/openbible/src/fetch.rs` makes it. The core calls it on its job threads, never on
/// the main thread, so it blocks until the answer is in. No cookie, cache, credential or
/// identifier beyond the app's version goes with a request; a request to GitHub's API asks
/// for its JSON format and API version, as GitHub documents. Release builds fetch `https`
/// only.
final class HTTPTransport: Transport, @unchecked Sendable {
    /// GitHub's REST API, whose requests ask for its JSON format (`fetch.rs` `GITHUB_API`).
    static let githubAPI = "https://api.github.com/"
    /// Bounds connecting and each wait for data; a slow but moving answer is never cut off.
    static let timeout: TimeInterval = 30

    private let session: URLSession

    init(timeout: TimeInterval = HTTPTransport.timeout) {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = timeout
        config.httpCookieStorage = nil
        config.httpShouldSetCookies = false
        config.urlCredentialStorage = nil
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        config.httpAdditionalHeaders = ["User-Agent": "OpenBible/\(OpenBibleCore.engineVersion())"]
        session = URLSession(configuration: config)
    }

    func get(url: String) throws -> Data {
        let scheme = url.components(separatedBy: "://").first?.lowercased() ?? ""
        #if !DEBUG
            // Release builds: https only (#333; the core's URLs are `https`). Debug tests read
            // a local mirror over http and `file://`.
            guard scheme == "https" else {
                throw TransportError.Network(message: "\(url): unsupported protocol \"\(scheme)\"")
            }
        #endif
        switch scheme {
        case "http", "https":
            break
        case "file":
            // As the core's own transport reads `file://` (a local mirror).
            guard let file = URL(string: url), FileManager.default.fileExists(atPath: file.path) else {
                throw TransportError.NotFound
            }
            do { return try Data(contentsOf: file) } catch { throw HTTPStatus.failure(url: url, error) }
        default:
            throw TransportError.Network(message: "\(url): unsupported protocol \"\(scheme)\"")
        }
        guard let target = URL(string: url) else {
            throw TransportError.Network(message: "\(url): not a URL")
        }
        var request = URLRequest(url: target)
        if url.hasPrefix(Self.githubAPI) {
            request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
            request.setValue("2022-11-28", forHTTPHeaderField: "X-GitHub-Api-Version")
        }
        let done = DispatchSemaphore(value: 0)
        nonisolated(unsafe) var answer: Result<Data, TransportError> = .failure(.Network(message: "\(url): no answer"))
        let task = session.dataTask(with: request) { data, response, error in
            if let error {
                answer = .failure(HTTPStatus.failure(url: url, error))
            } else if let refused = HTTPStatus.check(url: url, response: response) {
                answer = .failure(refused)
            } else {
                answer = .success(data ?? Data())
            }
            done.signal()
        }
        task.resume()
        done.wait()
        return try answer.get()
    }
}
