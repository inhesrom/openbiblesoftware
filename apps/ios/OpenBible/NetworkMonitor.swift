import Foundation
import Network
import Observation
import OpenBibleCore

/// The device's network for the core's download rules (`Network.is_wifi`, #333 "Wi-Fi only by
/// default"): from `NWPathMonitor`, a path counts as cellular when it uses the cellular
/// interface, and anything else (Wi-Fi, wired, no network at all) as not cellular, which is
/// what `URLSessionConfiguration.allowsCellularAccess = false` waits for. The core decides
/// what to do on each; this only reports which it is.
@MainActor
@Observable
final class NetworkMonitor {
    /// Not on a cellular path.
    private(set) var isWiFi = true
    @ObservationIgnored private let monitor = NWPathMonitor()
    @ObservationIgnored private var started = false
    /// A test's network instead of the device's (`OBS_TEST_NETWORK`, Debug builds).
    @ObservationIgnored private let injected: Bool?

    static let shared = NetworkMonitor()

    init(injected: Bool? = NetworkMonitor.testNetwork()) {
        self.injected = injected
        if let injected { isWiFi = injected }
    }

    /// Debug builds: `OBS_TEST_NETWORK=cellular` or `wifi` stands for the device's path, so
    /// a test can check Wi-Fi only on a cellular path the simulator does not have.
    nonisolated static func testNetwork() -> Bool? {
        #if DEBUG
            switch ProcessInfo.processInfo.environment["OBS_TEST_NETWORK"] {
            case "cellular": return false
            case "wifi": return true
            default: return nil
            }
        #else
            return nil
        #endif
    }

    func start() {
        guard !started, injected == nil else { return }
        started = true
        monitor.pathUpdateHandler = { [weak self] path in
            let wifi = !path.usesInterfaceType(.cellular)
            Task { @MainActor in self?.isWiFi = wifi }
        }
        monitor.start(queue: DispatchQueue(label: "io.github.inhesrom.openbible.network"))
    }

    /// The core's input: this network and the core's own "Use cellular data".
    func network(_ core: MobileApp) -> Network {
        Network(isWifi: isWiFi, allowCellular: (try? core.useCellular()) ?? false)
    }
}
