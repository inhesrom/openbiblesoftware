import OpenBibleCore
import UIKit

/// A text position for UIKit's text input: one of the core's text offsets
/// (`MobileApp.textEnd`, `closestOffset`); what an offset means is the core's.
final class TextOffsetPosition: UITextPosition {
    let offset: UInt64
    init(_ offset: UInt64) { self.offset = offset }
}

final class TextOffsetRange: UITextRange {
    let start_: TextOffsetPosition
    let end_: TextOffsetPosition
    init(_ a: UInt64, _ b: UInt64) {
        start_ = TextOffsetPosition(min(a, b))
        end_ = TextOffsetPosition(max(a, b))
    }
    override var start: UITextPosition { start_ }
    override var end: UITextPosition { end_ }
    override var isEmpty: Bool { start_.offset == end_.offset }
    var span: TextSpan { TextSpan(start: start_.offset, end: end_.offset) }
}

final class TextSelectionBox: UITextSelectionRect {
    private let box: CGRect
    private let starts: Bool
    private let ends: Bool
    init(_ box: CGRect, starts: Bool, ends: Bool) {
        self.box = box
        self.starts = starts
        self.ends = ends
    }
    override var rect: CGRect { box }
    override var writingDirection: NSWritingDirection { .natural }
    override var containsStart: Bool { starts }
    override var containsEnd: Bool { ends }
    override var isVertical: Bool { false }
}

/// Word, Verse and the other text units come from the core (`enclosingSpan`); the string
/// tokenizer's other answers (word boundaries while a handle is dragged) are UIKit's, over
/// `text(in:)`: an accepted prototype gap (`docs/dev/mobile-text-view.md`, S14a).
final class CoreTokenizer: UITextInputStringTokenizer {
    weak var canvas: TextCanvasView?

    init(canvas: TextCanvasView) {
        self.canvas = canvas
        super.init(textInput: canvas)
    }

    override func rangeEnclosingPosition(
        _ position: UITextPosition, with granularity: UITextGranularity,
        inDirection direction: UITextDirection
    ) -> UITextRange? {
        guard let canvas, let p = position as? TextOffsetPosition,
            let span = try? canvas.core?.enclosingSpan(
                reader: canvas.reader, offset: p.offset, kind: TextUnitKind(granularity))
        else { return nil }
        return TextOffsetRange(span.start, span.end)
    }
}

extension TextUnitKind {
    /// UIKit's granularity under the core's name for it.
    init(_ granularity: UITextGranularity) {
        switch granularity {
        case .character: self = .character
        case .word: self = .word
        case .sentence: self = .sentence
        case .paragraph: self = .paragraph
        case .line: self = .line
        case .document: self = .document
        @unknown default: self = .character
        }
    }
}

/// The page's content view: tile layers are its sublayers, and it is the text input that
/// `UITextInteraction` selects in (handles, loupe, the edit menu) and the accessibility
/// container of the Verses. Every answer comes from the core.
final class TextCanvasView: UIView, UITextInput {
    weak var owner: ResourceTextView?
    weak var selectionInteraction: UITextInteraction?
    var core: MobileApp? { owner?.core }
    var reader: UInt32 { owner?.reader ?? 0 }
    var selected: TextOffsetRange?
    weak var inputDelegate: UITextInputDelegate?
    private var accessibilityCache: [UIAccessibilityElement]?
    lazy var tokenizer: UITextInputTokenizer = CoreTokenizer(canvas: self)

    override var canBecomeFirstResponder: Bool { true }
    /// Read-only text: no keyboard.
    override var inputView: UIView? { UIView(frame: .zero) }

    /// The text moved or changed under the selection and the accessibility elements.
    func textMoved() {
        accessibilityCache = nil
    }

    func clearSelection() {
        inputDelegate?.selectionWillChange(self)
        selected = nil
        inputDelegate?.selectionDidChange(self)
        resignFirstResponder()
    }

    // MARK: Copy

    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        if action == #selector(copy(_:)) { return !(selected?.isEmpty ?? true) }
        return false
    }

    override func copy(_ sender: Any?) {
        guard let selected, let text = try? core?.copyText(reader: reader, span: selected.span)
        else { return }
        UIPasteboard.general.string = text
    }

    // MARK: UIKeyInput (read-only)

    var hasText: Bool { true }
    func insertText(_ text: String) {}
    func deleteBackward() {}

    // MARK: UITextInput

    func text(in range: UITextRange) -> String? {
        guard let r = range as? TextOffsetRange else { return nil }
        return try? core?.textIn(reader: reader, span: r.span)
    }

    func replace(_ range: UITextRange, withText text: String) {}

    var selectedTextRange: UITextRange? {
        get { selected }
        set {
            selected = newValue as? TextOffsetRange
        }
    }

    var markedTextRange: UITextRange? { nil }
    var markedTextStyle: [NSAttributedString.Key: Any]? {
        get { nil }
        set {}
    }
    func setMarkedText(_ markedText: String?, selectedRange: NSRange) {}
    func unmarkText() {}

    var beginningOfDocument: UITextPosition { TextOffsetPosition(0) }
    var endOfDocument: UITextPosition {
        TextOffsetPosition((try? core?.textEnd(reader: reader)) ?? 0)
    }

    func textRange(from: UITextPosition, to: UITextPosition) -> UITextRange? {
        guard let a = from as? TextOffsetPosition, let b = to as? TextOffsetPosition else {
            return nil
        }
        return TextOffsetRange(a.offset, b.offset)
    }

    func position(from position: UITextPosition, offset: Int) -> UITextPosition? {
        guard let p = position as? TextOffsetPosition,
            let moved = try? core?.moveOffset(reader: reader, offset: p.offset, by: Int64(offset))
        else { return nil }
        return TextOffsetPosition(moved)
    }

    func position(
        from position: UITextPosition, in direction: UITextLayoutDirection, offset: Int
    ) -> UITextPosition? {
        switch direction {
        case .left, .up: return self.position(from: position, offset: -offset)
        default: return self.position(from: position, offset: offset)
        }
    }

    func compare(_ position: UITextPosition, to other: UITextPosition) -> ComparisonResult {
        guard let a = position as? TextOffsetPosition, let b = other as? TextOffsetPosition else {
            return .orderedSame
        }
        return a.offset < b.offset ? .orderedAscending : a.offset > b.offset ? .orderedDescending : .orderedSame
    }

    func offset(from: UITextPosition, to toPosition: UITextPosition) -> Int {
        guard let a = from as? TextOffsetPosition, let b = toPosition as? TextOffsetPosition else {
            return 0
        }
        return Int(Int64(b.offset) - Int64(a.offset))
    }

    func position(within range: UITextRange, farthestIn direction: UITextLayoutDirection)
        -> UITextPosition?
    {
        switch direction {
        case .left, .up: return range.start
        default: return range.end
        }
    }

    func characterRange(byExtending position: UITextPosition, in direction: UITextLayoutDirection)
        -> UITextRange?
    {
        guard let other = self.position(from: position, in: direction, offset: 1) else { return nil }
        return textRange(from: position, to: other)
    }

    func baseWritingDirection(for position: UITextPosition, in direction: UITextStorageDirection)
        -> NSWritingDirection
    { .natural }
    func setBaseWritingDirection(_ writingDirection: NSWritingDirection, for range: UITextRange) {}

    private func rects(_ range: UITextRange) -> [CGRect] {
        guard let r = range as? TextOffsetRange,
            let rects = try? core?.spanRects(reader: reader, span: r.span)
        else { return [] }
        return rects.map { CGRect(x: CGFloat($0.x), y: $0.y, width: CGFloat($0.width), height: CGFloat($0.height)) }
    }

    func firstRect(for range: UITextRange) -> CGRect {
        rects(range).first ?? .null
    }

    func caretRect(for position: UITextPosition) -> CGRect {
        guard let p = position as? TextOffsetPosition,
            let r = try? core?.caretRect(reader: reader, offset: p.offset)
        else { return .null }
        return CGRect(x: CGFloat(r.x), y: r.y, width: CGFloat(r.width), height: CGFloat(r.height))
    }

    func selectionRects(for range: UITextRange) -> [UITextSelectionRect] {
        let all = rects(range)
        return all.enumerated().map { i, r in
            TextSelectionBox(r, starts: i == 0, ends: i == all.count - 1)
        }
    }

    func closestPosition(to point: CGPoint) -> UITextPosition? {
        guard let offset = try? core?.closestOffset(reader: reader, x: Float(point.x), y: Double(point.y))
        else { return nil }
        return TextOffsetPosition(offset)
    }

    func closestPosition(to point: CGPoint, within range: UITextRange) -> UITextPosition? {
        guard let p = closestPosition(to: point) as? TextOffsetPosition,
            let r = range as? TextOffsetRange
        else { return nil }
        return TextOffsetPosition(min(max(p.offset, r.start_.offset), r.end_.offset))
    }

    func characterRange(at point: CGPoint) -> UITextRange? {
        guard let p = closestPosition(to: point) else { return nil }
        return tokenizer.rangeEnclosingPosition(p, with: .character, inDirection: .storage(.forward))
    }

    var textInputView: UIView { self }

    // MARK: Accessibility: one element per Verse, from the core

    override var isAccessibilityElement: Bool {
        get { false }
        set {}
    }

    override var accessibilityElements: [Any]? {
        get {
            if let cached = accessibilityCache { return cached }
            let items = (try? core?.accessibilityItems(reader: reader)) ?? []
            let elements = items.map { item -> UIAccessibilityElement in
                let e = UIAccessibilityElement(accessibilityContainer: self)
                e.accessibilityLabel = item.label
                e.accessibilityIdentifier = item.id
                e.accessibilityTraits = item.heading ? [.staticText, .header] : .staticText
                e.accessibilityFrameInContainerSpace = CGRect(
                    x: CGFloat(item.frame.x), y: item.frame.y, width: CGFloat(item.frame.width),
                    height: CGFloat(item.frame.height))
                return e
            }
            accessibilityCache = elements
            return elements
        }
        set {}
    }
}
