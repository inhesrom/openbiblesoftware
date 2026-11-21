import OpenBibleCore
import SwiftUI
import UIKit
import os

/// S07 prototype (#331): Resource text drawn by the Rust pipeline as CPU tiles (plan A4,
/// option B) in a `UIScrollView`. The scroll view's content coordinates are the reader's page
/// in points; the core decides which tiles exist, where they go, what is under a point and
/// what a tap opens (`obs_mobile::text_view`). This view only shows images where it is told,
/// forwards the content offset and input, and measures.
final class ResourceTextView: UIScrollView, UIScrollViewDelegate {
    let core: MobileApp
    private(set) var reader: UInt32
    let canvas: TextCanvasView
    private var tiles: [TileKey: CALayer] = [:]
    private var applying = false
    private(set) var lastFrame: ReaderFrame?
    private static let log = Logger(subsystem: "io.github.inhesrom.openbible", category: "text")

    var onLocation: ((String) -> Void)?
    var onTap: ((StudySheetStub) -> Void)?
    var onFirstText: (() -> Void)?
    let metrics = TextViewMetrics()

    init(core: MobileApp, reader: UInt32) {
        self.core = core
        self.reader = reader
        self.canvas = TextCanvasView()
        super.init(frame: .zero)
        canvas.owner = self
        delegate = self
        contentInsetAdjustmentBehavior = .never
        showsHorizontalScrollIndicator = false
        alwaysBounceVertical = true
        addSubview(canvas)
        let tap = UITapGestureRecognizer(target: self, action: #selector(tapped(_:)))
        canvas.addGestureRecognizer(tap)
        let interaction = UITextInteraction(for: .nonEditable)
        interaction.textInput = canvas
        canvas.addInteraction(interaction)
        canvas.selectionInteraction = interaction
        registerForTraitChanges([UITraitUserInterfaceStyle.self, UITraitPreferredContentSizeCategory.self]) {
            (view: ResourceTextView, _: UITraitCollection) in
            view.followContentSize()
            view.refresh()
        }
        followContentSize()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// Show another reader (a new Resource, or after a jump the core already made).
    func show(reader: UInt32) {
        if reader != self.reader {
            self.reader = reader
            dropTiles()
            canvas.selected = nil
        }
        refresh()
    }

    /// The system text size, as the core maps it to a Text size.
    private func followContentSize() {
        _ = core.setContentSize(category: traitCollection.preferredContentSizeCategory.rawValue)
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        refresh()
    }

    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        refresh()
    }

    func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
        metrics.beginScroll()
    }

    func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) {
        if !decelerate { metrics.endScroll() }
    }

    func scrollViewDidEndDecelerating(_ scrollView: UIScrollView) {
        metrics.endScroll()
    }

    /// Ask the core for the frame at the current offset, move to the position it reports,
    /// and show its tiles.
    func refresh() {
        guard !applying, bounds.width > 0, bounds.height > 0, let scale = window?.screen.scale
        else { return }
        let maxOffset = max(0, contentSize.height - bounds.height)
        let offset = min(max(contentOffset.y, 0), maxOffset)
        // Rubber-banding past either end is the scroll view's own; the core sees the edge.
        let overscroll = contentOffset.y - offset
        let frame: ReaderFrame
        do {
            frame = try core.readerFrame(
                reader: reader, offset: Double(offset), width: Float(bounds.width),
                height: Float(bounds.height), scale: Float(scale),
                dark: traitCollection.userInterfaceStyle == .dark)
        } catch {
            Self.log.error("frame: \(error.localizedDescription, privacy: .public)")
            return
        }
        metrics.frameMicros(frame.micros)
        applying = true
        let height = max(CGFloat(frame.total), bounds.height)
        if abs(contentSize.height - height) > 0.5 || contentSize.width != bounds.width {
            contentSize = CGSize(width: bounds.width, height: height)
        }
        if canvas.frame.size != contentSize {
            canvas.frame = CGRect(origin: .zero, size: contentSize)
        }
        if abs(CGFloat(frame.position) - offset) > 0.01 {
            contentOffset.y = CGFloat(frame.position) + overscroll
        }
        applying = false
        backgroundColor = UIColor(
            red: CGFloat(frame.pageRed) / 255, green: CGFloat(frame.pageGreen) / 255,
            blue: CGFloat(frame.pageBlue) / 255, alpha: 1)
        place(frame, scale: scale)
        if lastFrame?.location != frame.location { onLocation?(frame.location) }
        lastFrame = frame
        canvas.textMoved()
    }

    private func dropTiles() {
        for layer in tiles.values { layer.removeFromSuperlayer() }
        tiles.removeAll()
    }

    /// Keep the tiles the frame lists, at its places; draw the missing ones that are in
    /// view now, and up to a few milliseconds' worth of the ones drawn ahead.
    private func place(_ frame: ReaderFrame, scale: CGFloat) {
        let keep = Set(frame.tiles.map(\.key))
        for (key, layer) in tiles where !keep.contains(key) {
            layer.removeFromSuperlayer()
            tiles[key] = nil
        }
        let visible = CGRect(x: 0, y: contentOffset.y, width: bounds.width, height: bounds.height)
        let started = ContinuousClock.now
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for place in frame.tiles {
            let rect = CGRect(x: 0, y: place.y, width: Double(bounds.width), height: Double(place.height))
            if let layer = tiles[place.key] {
                if layer.frame.origin != rect.origin { layer.frame.origin = rect.origin }
                continue
            }
            let needed = rect.intersects(visible)
            if !needed && ContinuousClock.now - started > .milliseconds(4) { continue }
            if needed && lastFrame != nil { metrics.visibleMiss() }
            guard let layer = makeTile(place, scale: scale) else { continue }
            layer.frame.origin = rect.origin
            canvas.layer.addSublayer(layer)
            tiles[place.key] = layer
        }
        CATransaction.commit()
        if !tiles.isEmpty, let first = onFirstText {
            onFirstText = nil
            first()
        }
    }

    private func makeTile(_ place: TilePlace, scale: CGFloat) -> CALayer? {
        let began = ContinuousClock.now
        guard let timed = try? core.readerTile(reader: reader, key: place.key, scale: Float(scale))
        else { return nil }
        let tile = timed.tile
        guard tile.width > 0, tile.height > 0,
            let provider = CGDataProvider(data: tile.rgba as CFData),
            let space = CGColorSpace(name: CGColorSpace.sRGB),
            let image = CGImage(
                width: Int(tile.width), height: Int(tile.height), bitsPerComponent: 8,
                bitsPerPixel: 32, bytesPerRow: Int(tile.width) * 4, space: space,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                provider: provider, decode: nil, shouldInterpolate: false,
                intent: .defaultIntent)
        else { return nil }
        let layer = CALayer()
        layer.contents = image
        layer.contentsScale = scale
        layer.contentsGravity = .topLeft
        layer.isOpaque = true
        layer.frame.size = CGSize(width: CGFloat(tile.width) / scale, height: CGFloat(tile.height) / scale)
        metrics.tile(rustMicros: timed.micros, total: ContinuousClock.now - began)
        return layer
    }

    @objc private func tapped(_ recognizer: UITapGestureRecognizer) {
        if let selected = canvas.selected, !selected.isEmpty {
            canvas.clearSelection()
            return
        }
        let at = recognizer.location(in: canvas)
        if let stub = try? core.tap(reader: reader, x: Float(at.x), y: Double(at.y)) {
            onTap?(stub)
        }
    }

    /// Scroll to where the core put the reader (after a jump).
    func adoptPosition() {
        refresh()
    }
}

/// What the prototype measures while scrolling: frames dropped (from `CADisplayLink`), tile
/// times, visible tiles that had to be drawn on the spot.
@MainActor
final class TextViewMetrics {
    private var link: CADisplayLink?
    private var last: CFTimeInterval = 0
    private(set) var frames = 0
    private(set) var dropped = 0
    private(set) var tileRustMicros: [UInt32] = []
    private(set) var tileTotalMicros: [Int] = []
    private(set) var frameMicrosList: [UInt32] = []
    private(set) var visibleMisses = 0
    private var scrolling = 0
    var onChange: (() -> Void)?

    func beginScroll() {
        scrolling += 1
        guard link == nil else { return }
        let link = CADisplayLink(target: DisplayLinkTarget(self), selector: #selector(DisplayLinkTarget.tick(_:)))
        link.add(to: .main, forMode: .common)
        self.link = link
        last = 0
    }

    func endScroll() {
        scrolling = max(0, scrolling - 1)
        guard scrolling == 0 else { return }
        link?.invalidate()
        link = nil
        onChange?()
    }

    func reset() {
        frames = 0
        dropped = 0
        tileRustMicros.removeAll()
        tileTotalMicros.removeAll()
        frameMicrosList.removeAll()
        visibleMisses = 0
    }

    fileprivate func tick(_ link: CADisplayLink) {
        defer { last = link.timestamp }
        guard last > 0 else { return }
        let period = link.targetTimestamp - link.timestamp
        guard period > 0 else { return }
        let missed = Int(((link.timestamp - last) / period).rounded()) - 1
        frames += 1 + max(0, missed)
        dropped += max(0, missed)
    }

    func tile(rustMicros: UInt32, total: Duration) {
        tileRustMicros.append(rustMicros)
        tileTotalMicros.append(Int(total.components.attoseconds / 1_000_000_000_000))
    }

    func frameMicros(_ micros: UInt32) {
        if scrolling > 0 { frameMicrosList.append(micros) }
    }

    func visibleMiss() { visibleMisses += 1 }

    var droppedPercent: Double { frames == 0 ? 0 : Double(dropped) * 100 / Double(frames) }

    func summary() -> String {
        func pct(_ xs: [Int], _ p: Double) -> Int {
            guard !xs.isEmpty else { return 0 }
            let s = xs.sorted()
            return s[min(s.count - 1, Int(Double(s.count) * p))]
        }
        let rust = tileRustMicros.map(Int.init)
        let frame = frameMicrosList.map(Int.init)
        return String(
            format: "frames %d dropped %d (%.2f%%) · tiles %d rust p50 %d µs p95 %d µs max %d µs · swift p50 %d µs p95 %d µs · frame p50 %d µs p95 %d µs · misses %d",
            frames, dropped, droppedPercent, rust.count, pct(rust, 0.5), pct(rust, 0.95),
            rust.max() ?? 0, pct(tileTotalMicros, 0.5), pct(tileTotalMicros, 0.95),
            pct(frame, 0.5), pct(frame, 0.95), visibleMisses)
    }
}

/// `CADisplayLink` holds its target strongly; this breaks the cycle.
@MainActor
private final class DisplayLinkTarget: NSObject {
    weak var metrics: TextViewMetrics?
    init(_ metrics: TextViewMetrics) { self.metrics = metrics }
    @objc func tick(_ link: CADisplayLink) { metrics?.tick(link) }
}

/// The SwiftUI host of the text view.
struct ResourceText: UIViewRepresentable {
    let model: ReaderModel

    func makeUIView(context: Context) -> ResourceTextView {
        let view = ResourceTextView(core: model.core!, reader: model.reader)
        model.attach(view)
        return view
    }

    func updateUIView(_ view: ResourceTextView, context: Context) {}
}
