import AppKit

final class OutlineClip: NSClipView { override var isFlipped: Bool { true } }

final class Editor: NSTextView {
    override func paste(_ sender: Any?) { pasteAsPlainText(sender) }
}

final class Document: NSDocument, NSTextViewDelegate {
    fileprivate let editor = Editor()
    private let outline = NSStackView()
    private let status = NSTextField(labelWithString: "")
    private let mode = NSSegmentedControl(labels: ["Write", "Source"], trackingMode: .selectOne, target: nil, action: nil)
    private var headings: [(String, Int)] = []
    private var lineEnding = "\n"
    private var bom = false
    private var source = false
    override class var autosavesInPlace: Bool { true }

    override func read(from data: Data, ofType typeName: String) throws {
        bom = data.starts(with: [0xef, 0xbb, 0xbf])
        guard let text = String(data: bom ? data.dropFirst(3) : data, encoding: .utf8) else { throw CocoaError(.fileReadInapplicableStringEncoding) }
        lineEnding = text.contains("\r\n") && !text.replacingOccurrences(of: "\r\n", with: "").contains("\n") ? "\r\n" : "\n"
        editor.string = lineEnding == "\r\n" ? text.replacingOccurrences(of: "\r\n", with: "\n") : text
        style()
    }
    override func data(ofType typeName: String) throws -> Data {
        let text = lineEnding == "\r\n" ? editor.string.replacingOccurrences(of: "\n", with: "\r\n") : editor.string
        return Data(((bom ? "\u{FEFF}" : "") + text).utf8)
    }
    override func makeWindowControllers() {
        let window = NSWindow(contentRect: NSRect(x: 0,y: 0,width: 940,height: 660), styleMask: [.titled,.closable,.miniaturizable,.resizable], backing: .buffered, defer: false)
        let scroll = NSScrollView(frame: window.contentView!.bounds)
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        editor.usesFindBar = true
        editor.isRichText = false
        editor.allowsUndo = true
        editor.isAutomaticQuoteSubstitutionEnabled = false
        editor.isAutomaticDashSubstitutionEnabled = false
        editor.isAutomaticTextReplacementEnabled = false
        editor.textContainerInset = NSSize(width: 42,height: 34)
        editor.isVerticallyResizable = true
        editor.autoresizingMask = [.width]
        editor.frame.size = scroll.contentSize
        editor.textContainer?.widthTracksTextView = true
        editor.delegate = self
        scroll.documentView = editor
        let root = NSView()
        let rail = NSVisualEffectView()
        rail.material = .sidebar
        rail.blendingMode = .withinWindow
        let caption = NSTextField(labelWithString: "Outline")
        caption.font = .systemFont(ofSize: 14, weight: .semibold)
        outline.orientation = .vertical
        outline.alignment = .leading
        outline.spacing = 7
        let navigation = NSScrollView()
        navigation.contentView = OutlineClip()
        navigation.drawsBackground = false
        navigation.hasVerticalScroller = true
        navigation.autohidesScrollers = true
        navigation.documentView = outline
        mode.selectedSegment = source ? 1 : 0
        mode.target = self
        mode.action = #selector(toggleSource(_:))
        mode.controlSize = .small
        status.font = .monospacedDigitSystemFont(ofSize: 11,weight: .regular)
        status.textColor = .secondaryLabelColor
        for view in [rail, scroll, status, mode] as [NSView] {
            root.addSubview(view)
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        for view in [caption,navigation] as [NSView] {
            rail.addSubview(view)
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        outline.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            rail.leadingAnchor.constraint(equalTo: root.leadingAnchor), rail.widthAnchor.constraint(equalToConstant: 176),
            rail.topAnchor.constraint(equalTo: root.topAnchor), rail.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            caption.leadingAnchor.constraint(equalTo: rail.leadingAnchor,constant: 18), caption.topAnchor.constraint(equalTo: rail.topAnchor,constant: 21),
            navigation.topAnchor.constraint(equalTo: caption.bottomAnchor,constant: 14), navigation.leadingAnchor.constraint(equalTo: rail.leadingAnchor,constant: 14),
            navigation.trailingAnchor.constraint(equalTo: rail.trailingAnchor,constant: -8), navigation.bottomAnchor.constraint(equalTo: rail.bottomAnchor),
            outline.leadingAnchor.constraint(equalTo: navigation.contentView.leadingAnchor), outline.topAnchor.constraint(equalTo: navigation.contentView.topAnchor),
            outline.widthAnchor.constraint(equalTo: navigation.contentView.widthAnchor),
            scroll.leadingAnchor.constraint(equalTo: rail.trailingAnchor), scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: root.topAnchor), scroll.bottomAnchor.constraint(equalTo: mode.topAnchor,constant: -8),
            mode.trailingAnchor.constraint(equalTo: root.trailingAnchor,constant: -16), mode.bottomAnchor.constraint(equalTo: root.bottomAnchor,constant: -8),
            status.leadingAnchor.constraint(equalTo: scroll.leadingAnchor,constant: 20), status.centerYAnchor.constraint(equalTo: mode.centerYAnchor)
        ])
        window.minSize = NSSize(width: 560,height: 300)
        window.contentView = root
        window.center()
        addWindowController(NSWindowController(window: window))
        window.makeFirstResponder(editor)
        style()
    }
    func textDidChange(_ notification: Notification) { style() }
    func textViewDidChangeSelection(_ notification: Notification) { style() }
    @objc func toggleSource(_ sender: Any?) { source = sender is NSSegmentedControl ? mode.selectedSegment == 1 : !source; mode.selectedSegment = source ? 1 : 0; style() }
    @objc func jump(_ sender: NSButton) {
        let range = NSRange(location: sender.tag,length: 0)
        editor.setSelectedRange(range)
        editor.scrollRangeToVisible(range)
        editor.window?.makeFirstResponder(editor)
    }
    @objc func inlineFormat(_ sender: NSMenuItem) {
        guard let marker = sender.representedObject as? String else { return }
        let range = editor.selectedRange()
        let selected = (editor.string as NSString).substring(with: range)
        editor.insertText(marker + selected + marker,replacementRange: range)
        editor.setSelectedRange(NSRange(location: range.location + marker.utf16.count,length: range.length))
    }
    @objc func toggleTask(_ sender: Any?) {
        let text = editor.string as NSString
        let range = text.lineRange(for: NSRange(location: editor.selectedRange().location,length: 0))
        let line = text.substring(with: range)
        let prefix: String
        let count: Int
        if line.hasPrefix("- [ ] ") { prefix = "- [x] "; count = 6 }
        else if line.lowercased().hasPrefix("- [x] ") { prefix = "- [ ] "; count = 6 }
        else { prefix = "- [ ] "; count = line.hasPrefix("- ") ? 2 : 0 }
        editor.insertText(prefix,replacementRange: NSRange(location: range.location,length: count))
    }
    @objc func formatBlock(_ sender: NSMenuItem) {
        let text = editor.string as NSString
        let line = text.lineRange(for: NSRange(location: editor.selectedRange().location,length: 0))
        let bytes = Array(text.substring(with: line).utf8)
        let count = bytes.withUnsafeBufferPointer { folio_prefix($0.baseAddress,$0.count) }
        editor.insertText(sender.representedObject as? String ?? "", replacementRange: NSRange(location: line.location,length: count))
    }
    private static let inline = try! NSRegularExpression(pattern: #"(?<![\\*_])([*]{2}|[_]{2}|[*]|[_]|`)(?=\S)(.+?)(?<=\S)\1"#)
    private func style() {
        guard let storage = editor.textStorage else { return }
        let text = editor.string as NSString
        let all = NSRange(location: 0,length: text.length)
        let body = NSFont.systemFont(ofSize: 17)
        editor.typingAttributes = [.font: body,.foregroundColor: NSColor.textColor]
        storage.beginEditing()
        defer { storage.endEditing() }
        storage.setAttributes([.font: body, .foregroundColor: NSColor.textColor], range: all)
        var entries: [(String, Int)] = []
        var index = 0
        var fence: (Character, Int)?
        while index < text.length {
            let range = text.lineRange(for: NSRange(location: index,length: 0))
            index = NSMaxRange(range)
            let line = text.substring(with: range)
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            let marker = trimmed.first
            let run = trimmed.prefix { $0 == marker }.count
            let delimiter = (marker == "`" || marker == "~") && run >= 3
            let code = fence != nil || delimiter
            if let (ch,n) = fence {
                if marker == ch && run >= n && run == trimmed.count { fence = nil }
            } else if delimiter, let marker { fence = (marker,run) }
            let bytes = Array(line.utf8)
            let count = bytes.withUnsafeBufferPointer { folio_prefix($0.baseAddress,$0.count) }
            if !code && count > 0 && line.first == "#" {
                entries.append((String(line.dropFirst(count)).trimmingCharacters(in: .whitespacesAndNewlines), range.location))
            }
            var font = body
            if source || code { font = .monospacedSystemFont(ofSize: 15,weight: .regular) }
            else if count > 0 && line.first == "#" { font = .systemFont(ofSize: CGFloat(34 - 3 * (count - 2)),weight: .semibold) }
            let paragraph = NSMutableParagraphStyle()
            paragraph.paragraphSpacing = source ? 0 : 4
            paragraph.lineSpacing = source ? 1 : 2
            storage.addAttributes([.font: font,.paragraphStyle: paragraph],range: range)
            if !source && code { storage.addAttribute(.backgroundColor,value: NSColor.quaternaryLabelColor,range: range) }
            if !source && !code {
                for match in Self.inline.matches(in: line, range: NSRange(location: 0,length: (line as NSString).length)) {
                    let mark = (line as NSString).substring(with: match.range(at: 1))
                    let span = NSRange(location: range.location + match.range.location,length: match.range.length)
                    let face = mark == "`" ? NSFont.monospacedSystemFont(ofSize: 16,weight: .regular) :
                        NSFontManager.shared.convert(font,toHaveTrait: mark.count == 2 ? .boldFontMask : .italicFontMask)
                    storage.addAttribute(.font,value: face,range: span)
                    if !NSLocationInRange(editor.selectedRange().location,span) {
                        for start in [span.location,NSMaxRange(span) - mark.count] {
                            let delimiter = NSRange(location: start,length: mark.count)
                            storage.addAttributes([.foregroundColor: NSColor.clear,.font: NSFont.systemFont(ofSize: 0.1)],range: delimiter)
                        }
                    }
                }
            }
            if !source && !code && count > 0 {
                let prefix = NSRange(location: range.location,length: count)
                let active = NSLocationInRange(editor.selectedRange().location,range) || editor.selectedRange().location == text.length && !line.hasSuffix("\n") && index == text.length
                if !active && line.first == "#" { storage.addAttribute(.font,value: NSFont.systemFont(ofSize: 0.1),range: prefix) }
                storage.addAttribute(.foregroundColor,value: active || line.first != "#" ? NSColor.secondaryLabelColor : NSColor.clear,range: prefix)
            }
        }
        status.stringValue = "\(editor.string.split(whereSeparator: { $0.isWhitespace }).count) words"
        if !entries.elementsEqual(headings,by: { $0 == $1 }) {
            headings = entries
            for view in outline.arrangedSubviews { outline.removeArrangedSubview(view); view.removeFromSuperview() }
            for (name,offset) in entries {
                let button = NSButton(title: name,target: self,action: #selector(jump(_:)))
                button.isBordered = false
                button.alignment = .left
                button.lineBreakMode = .byTruncatingTail
                button.toolTip = name
                button.tag = offset
                outline.addArrangedSubview(button)
                button.widthAnchor.constraint(lessThanOrEqualTo: outline.widthAnchor).isActive = true
            }
        }
    }
}

final class App: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        let bar = NSMenu()
        func menu(_ title: String) -> NSMenu {
            let item = NSMenuItem()
            bar.addItem(item)
            let menu = NSMenu(title: title)
            item.submenu = menu
            return menu
        }
        let app = menu("Folio")
        app.addItem(withTitle:"Quit Folio",action:#selector(NSApplication.terminate(_:)),keyEquivalent:"q")
        let file = menu("File")
        for (name,action,key) in [("New","newDocument:","n"),("Open…","openDocument:","o"),("Save","saveDocument:","s"),("Close","performClose:","w")] {
            file.addItem(withTitle:name,action:Selector(action),keyEquivalent:key)
        }
        let edit = menu("Edit")
        for (name,action,key) in [("Undo","undo:","z"),("Redo","redo:","Z"),("Cut","cut:","x"),("Copy","copy:","c"),("Paste","paste:","v"),("Select All","selectAll:","a")] {
            edit.addItem(withTitle:name,action:Selector(action),keyEquivalent:key)
        }
        let find = edit.addItem(withTitle:"Find…",action:#selector(NSTextView.performFindPanelAction(_:)),keyEquivalent:"f")
        find.tag = NSTextFinder.Action.showFindInterface.rawValue
        let format = menu("Format")
        for (name,mark,key) in [("Bold","**","b"),("Italic","*","i"),("Inline Code","`","e")] {
            format.addItem(withTitle:name,action:#selector(Document.inlineFormat(_:)),keyEquivalent:key).representedObject = mark
        }
        let task = format.addItem(withTitle:"Toggle Task",action:#selector(Document.toggleTask(_:)),keyEquivalent:"t")
        task.keyEquivalentModifierMask = [.command,.shift]
        for (name,prefix,key) in [("Paragraph","","0"),("Heading 1","# ","1"),("Heading 2","## ","2"),("Heading 3","### ","3"),("Bullet List","- ",""),("Quote","> ","")] {
            format.addItem(withTitle:name,action:#selector(Document.formatBlock(_:)),keyEquivalent:key).representedObject = prefix
        }
        format.addItem(withTitle:"Toggle Markdown Source",action:#selector(Document.toggleSource(_:)),keyEquivalent:"/")
        NSApp.mainMenu = bar
        NSApp.activate(ignoringOtherApps: true)
    }
}
let app = NSApplication.shared
if CommandLine.arguments.contains("--self-test") {
    let fixture = Data("\u{FEFF}# Héllo\r\n\r\n- **bold** and `code`\r\n".utf8)
    let doc = Document()
    try doc.read(from: fixture, ofType: "Markdown")
    doc.makeWindowControllers()
    let saved = try doc.data(ofType: "Markdown")
    precondition(saved == fixture)
    let view = doc.editor
    let font = view.textStorage!.attribute(.font, at: 2, effectiveRange: nil) as! NSFont
    precondition(font.pointSize == 34)
    view.setSelectedRange(NSRange(location: 2,length: 0))
    let item = NSMenuItem()
    item.representedObject = "## "
    doc.formatBlock(item)
    let changed = try doc.data(ofType: "Markdown")
    precondition(String(decoding: changed,as: UTF8.self).hasPrefix("\u{FEFF}## Héllo\r\n"))
    doc.toggleTask(nil)
    precondition(view.string.hasPrefix("- [ ] "))
    doc.toggleTask(nil)
    precondition(view.string.hasPrefix("- [x] "))
    let word = (view.string as NSString).range(of:"Héllo")
    view.setSelectedRange(word)
    item.representedObject = "**"
    doc.inlineFormat(item)
    precondition(view.string.contains("**Héllo**"))
    print("Document round-trip, heading rendering and block editing passed")
    exit(0)
}
let delegate = App()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
