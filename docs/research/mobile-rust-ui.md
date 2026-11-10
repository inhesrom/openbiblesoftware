# Rust UI on iOS and Android (#325 spike)

How should the "Open Bible" phone and tablet apps draw their interface? Two paths: run the existing egui
UI on mobile through eframe (winit + wgpu/glow), or write native SwiftUI and Jetpack Compose shells over
the Rust engine, with bindings from UniFFI or a similar tool. A middle path keeps a native shell and puts a
Rust-drawn view inside it. Checked against upstream repos, docs.rs, Android and Apple developer docs and
store-policy pages during this session (November 2026). The newest releases seen were egui/eframe 0.37.0
(2026-10-15), winit 0.30.x, accesskit_winit 0.34.2, accesskit_android 0.9.1 and accesskit_ios 0.2.2.

Our starting point (read from the repo, not upstream): the workspace pins `egui = "0.37"` and
`eframe = "0.37"` (default features off) and enables `glow`, `wayland` and `x11` in `crates/ui-egui`. It
uses `rfd 0.17` for file dialogs and sets `unsafe_code = "forbid"` across the workspace
([Cargo.toml](../../Cargo.toml), [crates/ui-egui/Cargo.toml](../../crates/ui-egui/Cargo.toml)). `obs-render`
(L3, wasm32-clean) rasterizes glyphs into a shared CPU **glyph atlas** and emits **quads**. Its rasterizer
is our own signed-area (font-rs-style) coverage rasterizer, built on `skrifa` with no vello dependency. The
UI layer uploads the atlas and paints the quads ([crates/render/src/lib.rs](../../crates/render/src/lib.rs),
[raster.rs](../../crates/render/src/raster.rs)). The ticket text says vello_cpu; the code does not use it.

## Capability matrix

| Concern | egui / eframe on mobile (0.37, winit 0.30) | Native shell (SwiftUI / Compose) over Rust core |
|---|---|---|
| **Lifecycle / GPU surface** | eframe drops the window and surface on `suspended` and rebuilds them on `resumed`, for both wgpu and glow, on Android only ([wgpu_integration.rs](https://raw.githubusercontent.com/emilk/egui/main/crates/eframe/src/native/wgpu_integration.rs), [glow_integration.rs](https://raw.githubusercontent.com/emilk/egui/main/crates/eframe/src/native/glow_integration.rs)). State is saved on suspend for Android and iOS (eframe 0.31, [#5601](https://github.com/emilk/egui/blob/main/crates/eframe/CHANGELOG.md)). One open bug: `on_exit` is not called on Android ([#5492](https://github.com/emilk/egui/labels/android)) | The OS owns the lifecycle. Rust only sees calls. A Rust-drawn sub-view must handle its own surface (Compose `AndroidExternalSurface` gives a `SurfaceScope` with surface lifecycle callbacks: [docs](https://developer.android.com/reference/kotlin/androidx/compose/foundation/AndroidExternalSurface.composable)) |
| **Soft keyboard / IME** | winit: `set_ime_allowed` shows and hides the soft keyboard on iOS and Android. `set_ime_cursor_area` and `set_ime_purpose` are **unsupported** on iOS and Android ([winit Window docs](https://docs.rs/winit/latest/winit/window/struct.Window.html)). Android text input needed a fix in egui 0.32 ([#5759](https://github.com/emilk/egui/pull/5759)). "Handle input method visibility for android" and "Mobile keyboard should not behave like IME" are still open ([#3890](https://github.com/emilk/egui/issues/3890), [#4573](https://github.com/emilk/egui/issues/4573)). egui 0.36's "drastically improved" mobile keyboard applies to **eframe web** only ([CHANGELOG](https://raw.githubusercontent.com/emilk/egui/main/CHANGELOG.md)) | System text fields give autocorrect, dictation and IME for free. A custom-drawn text view on iOS adopts `UITextInput` plus `UITextInteraction`, which the system documents as adding all system selection gestures ([Apple](https://developer.apple.com/documentation/uikit/uitextinteraction), [handleViews](https://developer.apple.com/documentation/uikit/uitextselectiondisplayinteraction/handleviews)). On Android it implements `InputConnection` through `InputMethodManager` ([Android](https://developer.android.com/reference/android/view/inputmethod/InputMethodManager)) |
| **Selection handles** | egui has no touch selection handles or loupe. Its selection is mouse-style (0.37 added double- and triple-click-drag: [CHANGELOG](https://raw.githubusercontent.com/emilk/egui/main/CHANGELOG.md)). The mobile text-editing tracking issue is open ([#4500](https://github.com/emilk/egui/issues/4500)) | Compose: `SelectionContainer` selects across composables ([docs](https://developer.android.com/develop/ui/compose/text/user-interactions)). iOS: `UITextInteraction` handles in custom views. SwiftUI `.textSelection` on iOS has historically selected the whole `Text`, not a range (secondary sources only; see Gaps) |
| **Screen readers** | egui emits an AccessKit tree, and 0.37 improved it ([CHANGELOG](https://raw.githubusercontent.com/emilk/egui/main/CHANGELOG.md)). `accesskit_winit` targets every AccessKit platform. On Android it **only works with GameActivity** ([docs.rs](https://docs.rs/crate/accesskit_winit/latest)). eframe refuses to compile `accesskit` with NativeActivity ([#6855](https://github.com/emilk/egui/pull/6855)). The iOS adapter (`UIAccessibility`, NLnet-funded) is young (0.2.2: [docs.rs](https://docs.rs/crate/accesskit_ios/latest)). AccessKit says its adapters "don't yet support rich text or hypertext" ([README](https://github.com/AccessKit/accesskit)) | Native widgets are accessible by default. A custom-drawn view needs `ExploreByTouchHelper` (virtual nodes: [docs](https://developer.android.com/reference/androidx/customview/widget/ExploreByTouchHelper)) or `UIAccessibilityElement`s. Alternatively, the `InjectingAdapter` in `accesskit_android` can inject a Rust AccessKit tree into **any Android View** without subclassing it ([docs.rs](https://docs.rs/crate/accesskit_android/latest)) |
| **Safe areas / rotation** | iOS safe area: egui 0.33 added `SafeAreaInsets` and `content_rect` ([#7578](https://github.com/emilk/egui/pull/7578)); 0.37 preserves insets ([CHANGELOG](https://raw.githubusercontent.com/emilk/egui/main/CHANGELOG.md)). The PR points to a proper safe-area API in **winit 0.31** (unreleased). Android insets are not in eframe: Grim reads `WindowInsetsCompat` in Java and passes them to Rust over JNI ([Grim MainActivity](https://github.com/ardocrat/grim/blob/master/android/app/src/main/java/mw/gri/android/MainActivity.java)) | The OS handles both (`safeAreaInsets` / `WindowInsets`). Size classes and split view are free |
| **Touch / scroll physics** | Drag-to-scroll is touch-only (0.35). Kinetic scrolling with rubber-banding arrived in 0.37 ([#8662/#8663](https://raw.githubusercontent.com/emilk/egui/main/CHANGELOG.md), [`KineticScrollStyle` in scroll_area.rs](https://raw.githubusercontent.com/emilk/egui/main/crates/egui/src/containers/scroll_area.rs)). It is tuned by egui, not by iOS or Android. Android back button: egui-winit 0.32 ([#7073](https://github.com/emilk/egui/blob/main/crates/egui-winit/CHANGELOG.md)) | Platform scroll physics, edge-swipe back, sheets and haptics |
| **File pickers / share sheet** | `rfd` supports Windows, macOS, Linux and WASM only, so **no mobile** ([docs.rs](https://docs.rs/rfd/latest/rfd/)). Grim hand-writes JNI for the file picker, clipboard and "open with" intents ([MainActivity](https://github.com/ardocrat/grim/blob/master/android/app/src/main/java/mw/gri/android/MainActivity.java)) | Direct calls: Android `ACTION_OPEN_DOCUMENT_TREE` plus `takePersistableUriPermission` for a user-chosen folder ([docs](https://developer.android.com/training/data-storage/shared/documents-files)). iOS `UIDocumentPickerViewController` with security-scoped bookmarks and file coordination ([Apple archive guide](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/DocumentPickerProgrammingGuide/AccessingDocuments/AccessingDocuments.html)) |
| **Build tooling** | `cargo-mobile2` has an `egui` template (egui + winit + wgpu, based on agdk-egui). Its maintainers say they support **only Tauri's use** ([README](https://github.com/tauri-apps/cargo-mobile2)). Notedeck uses `cargo apk` ([README](https://github.com/damus-io/notedeck)). Grim uses Gradle + GameActivity | `cargo-ndk` builds `.so` files into a `jniLibs` layout ([README](https://github.com/bbqsrc/cargo-ndk)). UniFFI generates Kotlin (over JNA) and Swift bindings ([Gradle](https://mozilla.github.io/uniffi-rs/latest/kotlin/gradle.html), [Xcode](https://mozilla.github.io/uniffi-rs/latest/swift/xcode.html)). `uniffi-bindgen-swift` writes XCFramework-ready modulemaps ([guide](https://mozilla.github.io/uniffi-rs/latest/swift/uniffi-bindgen-swift.html)). `cargo-swift` packages a Swift Package ([repo](https://github.com/antoniusnaumann/cargo-swift)) |
| **Binary size** | One data point: an egui Android hello world was 151 MB in debug and about 5 MB in release with `strip = "debuginfo"` ([#2066](https://github.com/emilk/egui/issues/2066)). Grim uses `opt-level="z"`, LTO, `codegen-units=1` and `panic="abort"` ([Cargo.toml](https://github.com/ardocrat/grim/blob/master/Cargo.toml)) | Rust core only (engine, rusqlite bundled, harfrust/skrifa) plus a platform UI that costs nothing extra on the device. No first-party numbers found; measure |

Both paths: from **2027-02-01** Google Play will refuse updates whose native libraries do not support 16 KB
pages on 64-bit devices. NDK r28+ aligns to 16 KB by default; r27 and older need
`-Wl,-z,max-page-size=16384`. AGP 8.5.1+ is needed for uncompressed, aligned `.so` files
([Android Developers](https://developer.android.com/guide/practices/page-sizes)).

## Findings

1. **eframe on mobile works, but iOS is second-class.** emilk wrote "Both iOS and Android work with eframe
   now", with CI compile checks (2024-09). The first reply was "just missing vital platform integrations like
   keyboard" ([#2066](https://github.com/emilk/egui/issues/2066), still open). Android became official in
   eframe 0.30 (2024-12-16) ([CHANGELOG](https://github.com/emilk/egui/blob/main/crates/eframe/CHANGELOG.md)).
   The eframe README still lists native targets as "Linux, Mac, Windows, and Android", without iOS
   ([README](https://github.com/emilk/egui/blob/main/crates/eframe/README.md)). Every egui fix for iOS and
   Android since then has been community-driven (safe area, back button, text input).

2. **Lifecycle is handled, with Android-only branches.** On Android, eframe's wgpu path calls
   `painter.set_window(ROOT, None)` on `suspended` and `recreate_window` on `resumed`. The glow path drops
   `gl_surface` and the window on suspend ("only applies for android")
   ([wgpu](https://raw.githubusercontent.com/emilk/egui/main/crates/eframe/src/native/wgpu_integration.rs),
   [glow](https://raw.githubusercontent.com/emilk/egui/main/crates/eframe/src/native/glow_integration.rs)).
   winit documents that on Android `suspended`/`resumed` track whether the render surface exists, not
   `onPause`/`onResume`, and that the surface must be dropped before the callback returns
   ([winit ApplicationHandler](https://docs.rs/winit/latest/aarch64-linux-android/winit/application/trait.ApplicationHandler.html)).
   Our app uses **glow**. eframe notes that its glow `AppCreator` is `FnOnce` and so "unable to support
   re-initializing … on Android if the application suspends and resumes" (it keeps the app object and only
   rebuilds the surface).

3. **Text input and selection are the biggest gap on the egui path.** winit gives only show/hide of the
   soft keyboard on mobile. It has no cursor-area or input-purpose support
   ([winit](https://docs.rs/winit/latest/winit/window/struct.Window.html)). Android IME events in winit were
   an experimental branch that never landed ([winit #2766](https://github.com/rust-windowing/winit/issues/2766)).
   Accented characters arriving as backspaces is still open ([winit #4067](https://github.com/rust-windowing/winit/issues/4067)).
   In practice, Grim overrides GameActivity's `onTextInputEventNative` in Java and forwards **one character at
   a time** to Rust over JNI ([MainActivity](https://github.com/ardocrat/grim/blob/master/android/app/src/main/java/mw/gri/android/MainActivity.java)).
   For a reading-first app, the hard part is **selecting Verses and words to annotate** with handles and a
   loupe. egui has no touch handles at all ([#4500](https://github.com/emilk/egui/issues/4500)).

4. **Screen-reader support on the egui path depends on young adapters and fixes the Activity type.**
   `accesskit_android` first shipped in 2025-03 and is now at 0.9.1. `accesskit_ios` is at 0.2.2
   ([android](https://docs.rs/crate/accesskit_android/latest), [ios](https://docs.rs/crate/accesskit_ios/latest)).
   `accesskit_winit` needs **GameActivity** on Android ([docs.rs](https://docs.rs/crate/accesskit_winit/latest)).
   Without it, AccessKit panics at runtime, which is why eframe made it a `compile_error!`
   ([#6855](https://github.com/emilk/egui/pull/6855)). AccessKit's own README says rich text and hypertext
   are not supported yet ([README](https://github.com/AccessKit/accesskit)), and Resource text is rich text.
   App Store Connect now publishes **Accessibility Nutrition Labels** with VoiceOver evaluation criteria
   ([Apple](https://developer.apple.com/help/app-store-connect/manage-app-accessibility/voiceover-evaluation-criteria/)),
   so VoiceOver support is visible on the product page.

5. **Platform services need hand-written glue on the egui path.** `rfd` has no mobile backend
   ([docs.rs](https://docs.rs/rfd/latest/rfd/)). Our sync design needs a **persistent, user-chosen cloud
   folder**. On Android that means SAF tree URIs with persisted permission
   ([docs](https://developer.android.com/training/data-storage/shared/documents-files)). On iOS it means
   security-scoped bookmarks with `NSFileCoordinator`
   ([Apple](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/DocumentPickerProgrammingGuide/AccessingDocuments/AccessingDocuments.html)).
   Both are Kotlin/Swift APIs whose files are not plain POSIX paths. Every app on either path writes this
   glue; the egui path also writes the UI glue (sheets, share, insets) by hand.

6. **Native shell over a Rust core is the established pattern for shipping apps.**
   - **Element X** (iOS and Android): SwiftUI and Jetpack Compose over `matrix-rust-sdk` through an FFI layer
     packaged as a Swift package ([element-x-ios](https://github.com/element-hq/element-x-ios/),
     [element-x-android](https://github.com/element-hq/element-x-android/),
     [Apple bindings](https://github.com/matrix-org/matrix-rust-sdk/blob/main/bindings/apple/README.md)).
   - **Firefox** for Android and iOS: `application-services` components "have their FFI generated by
     uniffi" ([README](https://github.com/mozilla/application-services)).
   - **Bitwarden** mobile: `bitwarden-uniffi` provides Swift and Kotlin bindings
     ([sdk-internal](https://github.com/bitwarden/sdk-internal)).
   - **Ferrostar** navigation SDK ([Stadia Maps](https://stadiamaps.com/blog/ferrostar-building-a-cross-platform-navigation-sdk-in-rust-part-1/)).
   - **Crux** formalises this split: a side-effect-free Rust core (it must compile to Wasm) and a thin
     SwiftUI/Compose shell, now packaged with BoltFFI ([repo](https://github.com/redbadger/crux)).

   Our engine is already wasm32-clean below L6, which matches Crux's core constraint.

7. **Real egui mobile apps exist, on Android only.**
   - **Notedeck** (Damus's Nostr client) runs on Android ([README](https://github.com/damus-io/notedeck)).
   - **Grim** (Grin wallet) is egui 0.33 on GameActivity with a large Java shim for insets, keyboard,
     clipboard, file picker, back button and camera ([repo](https://github.com/ardocrat/grim)).

   We found **no full-screen eframe app shipped on the iOS App Store** (see Gaps).

8. **The hybrid path (native shell with a Rust-drawn view inside) has shipped.** Lockbook embeds an
   egui/wgpu editor in SwiftUI through `MTKView` and in Android through a custom View. It routes iOS
   `UITextInput` and Android `InputConnection` events into Rust
   ([Lockbook's editor](https://parth.cafe/lb/editor.html)). Its Android client has `WorkspaceView.kt`,
   `WorkspaceTextInputConnection.kt` and `WorkspaceTouchForwarder.kt`
   ([repo](https://github.com/lockbook/lockbook)). jinleili's `wgpu-in-app` shows a wgpu surface inside a
   native iOS view and an Android `SurfaceView` (`WGPUSurfaceView.kt`)
   ([repo](https://github.com/jinleili/wgpu-in-app)). wgpu accepts a `CAMetalLayer` directly
   (`SurfaceTargetUnsafe::CoreAnimationLayer`) or a raw `UiKit`/`AndroidNdk` handle
   ([wgpu surface.rs](https://github.com/gfx-rs/wgpu/blob/trunk/wgpu/src/api/surface.rs)).
   `swiftui-egui-demo` packages egui as an XCFramework inside SwiftUI
   ([repo](https://github.com/alex566/swiftui-egui-demo)).

9. **Other Rust UI toolkits are similar to egui on mobile.** Slint runs on iOS (Rust only, winit + Skia) and
   Android (android-activity), and added safe-area and virtual-keyboard areas in 1.15
   ([iOS](https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/ios/),
   [Android](https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/android/),
   [1.15](https://www.slint.dev/blog/slint-1.15-released)). It would be a rewrite of `ui-egui` with the same
   class of gaps, so we did not evaluate it further.

## Hosting our Resource-text pipeline in a native view

The pipeline's output is a CPU glyph atlas plus a list of quads, so any of these hosts works without
changing `obs-layout` or `obs-render`:

| Option | How | Cost / risk |
|---|---|---|
| **A. GPU surface view** | iOS: `UIViewRepresentable` wrapping a `CAMetalLayer`/`MTKView`, then wgpu `CoreAnimationLayer`. Android: Compose `AndroidExternalSurface` or a `SurfaceView` passing an `ANativeWindow` to wgpu. Upload the atlas as a texture and draw the quads (or reuse `egui-wgpu` just for the reading pane, as Lockbook does) | Best scrolling at 120 Hz. We must handle surface create/destroy ourselves ([AndroidExternalSurface](https://developer.android.com/reference/kotlin/androidx/compose/foundation/AndroidExternalSurface.composable)). Needs `unsafe` (raw handles); `AndroidExternalSurface` sits behind the window by default (z-order with sheets) |
| **B. CPU bitmap** | Composite the quads into an RGBA buffer in Rust (the atlas is already CPU). Android: `AndroidBitmap_lockPixels` on a `Bitmap` ([NDK](https://developer.android.com/ndk/reference/group/bitmap)) or a byte array. iOS: `CGImage` from bytes. Show per page or per tile in a native scroll view | Simplest, no GPU lifecycle, wasm-friendly. Copies per tile; fine for page or tile caching, weaker for live pinch-zoom |
| **C. Native text** | Hand native text views `AttributedString` / `AnnotatedString` built from our layout | Native selection and VoiceOver for free, but **gives up our shaping, justification and bidi (ADR 0004)**, so the platforms would lay out differently from desktop |

With A or B, selection handles and accessibility still come from the host: `UITextInteraction` plus
`UITextInput` on iOS; on Android, custom handles, `ExploreByTouchHelper`, or AccessKit's `InjectingAdapter`
fed from our layout's Verse and word boxes.

## Implications for Open Bible mobile

- A reading-first app depends on **selecting text with handles, the soft keyboard (Notes, search),
  VoiceOver/TalkBack, safe areas, a native folder picker and a share sheet**. Those are exactly where eframe
  is weakest on mobile today: open issues #3890, #4500 and #4573; no Android insets; no mobile `rfd`;
  AccessKit with no rich text.
- The lowest-risk path matches what shipping apps do: **native SwiftUI and Compose shells over the Rust
  engine through UniFFI**, with **Resource text drawn by our pipeline in a hosted view** (option A or B).
  This keeps ADR 0004's identical rendering across desktop and mobile. Lockbook proves the shape works.
- The full-eframe path reuses the most code (`ui-egui`), but needs GameActivity, a Java/Swift shim per
  platform (Grim shows the size of it), winit 0.31 for safe areas, and our own touch-selection work. No App
  Store precedent was found.
- Either way: build with NDK r28+ for 16 KB pages before 2027-02-01. Set a lint policy for the FFI crate,
  since `unsafe_code = "forbid"` cannot cover raw-handle surface creation or JNI. Add `cargo-ndk` plus an
  XCFramework step (UniFFI / `cargo-swift`) to CI and to the GitHub Releases APK build.

## Open questions for the decision ticket

1. Native shells (two UI codebases, Swift + Kotlin) or one eframe UI with platform shims? Who maintains the
   Swift and Kotlin code?
2. Host Resource text on a GPU surface (A) or as CPU tiles (B)? Prototype B first, since it is wasm-friendly
   and avoids surface lifecycle code.
3. Selection UX: is `UITextInteraction` on a custom view (iOS) plus custom handles (Android) acceptable, or
   do we need native text (C) for selection only?
4. Accessibility bar: do we claim VoiceOver/TalkBack on the App Store nutrition label at 1.0? If so, use
   AccessKit's `InjectingAdapter` / iOS adapter or hand-written `UIAccessibilityElement`s?
5. Bindings: UniFFI (proc-macro), or Crux/BoltFFI's typed message core? How does that fit
   `unsafe_code = "forbid"`?
6. Tablet split view: native `NavigationSplitView` / `ListDetailPaneScaffold`, or reuse `egui_tiles`?

## Gaps

- No measured binary sizes for our engine on `aarch64-linux-android` or `aarch64-apple-ios`. Next step:
  `cargo ndk -t arm64-v8a build --release` of `obs-engine` with `rusqlite` bundled, then strip and record.
- We could not verify a **full-screen eframe app on the iOS App Store**. Searches turned up none, and
  Lockbook is a hybrid.
- AccessKit iOS through `egui-winit` was not tested on a device; the claim rests on the `accesskit_winit`
  docs.
- Apple's developer pages need JavaScript and could not be fetched (UITextInteraction, textSelection,
  XCFramework). Claims about SwiftUI `.textSelection` behaviour on iOS 26/27 come from secondary sources and
  need a device check.
- The Lockbook store listings and current egui version were not re-verified; the blog post is undated
  (it announces the editor shipping in 0.7.5).
