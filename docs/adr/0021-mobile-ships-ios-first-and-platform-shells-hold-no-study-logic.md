# Mobile ships iOS first, and platform shells hold no study logic

The mobile map (#324) began as one v1 for iOS and Android together. The owner chose to focus on
iOS and keep Android cheap to follow, and the UI-stack research (#325) found eframe weakest
exactly where a reading app needs the most on a phone (touch selection, the keyboard, VoiceOver,
safe areas, native pickers and share sheets). We decided:

1. **iOS ships first.** Mobile v1 is an iOS (iPhone and iPad) app. Android follows in the next
   release, with no fixed date; Play and the signed APK (#327) move with it.
2. **Each platform gets a native shell** (SwiftUI on iOS, Compose on Android) over the Rust
   engine through UniFFI. Resource text is still drawn by our own text pipeline (ADR 0004) in a
   view the shell hosts. SwiftUI is provisional until the stack prototype (#331) proves the
   hosted text view; a single cross-platform shell (Flutter or Compose Multiplatform) is the
   fallback.
3. **Shells hold no study logic.** What each screen shows, what an action does and every rule
   about Resources, Annotations, Search, Guides and sync lives in a shared Rust layer below the
   shells; a shell only lays out, draws and forwards input. Android is then "draw the same
   screens in Compose", not a port.

## Considered Options

- **iOS and Android together in native shells:** twice the shell work before anything ships.
- **One cross-platform shell (Flutter, Compose Multiplatform) from the start:** cheapest in
  total, but our text view hosted inside another framework's view makes selection and
  accessibility harder, and the iOS feel is less native. Kept as the fallback.
- **eframe everywhere, reusing `ui-egui`:** cheapest to start, weakest on touch, IME and
  screen readers, with no egui app found on the App Store.

## Consequences

- The shared Rust layer is a new crate above `engine` with UniFFI bindings; it needs an
  exception to `unsafe_code = "forbid"` for the FFI crate only.
- Building and running the iOS app needs macOS and Xcode; Linux can build and test the Rust
  layer and generate the Swift bindings.
- A rule that sits in Swift instead of Rust is a bug, because Android would have to copy it.
