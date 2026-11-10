# Store policy and mobile builds (#327)

What the Apple App Store and Google Play require of the planned "Open Bible" iOS and Android
apps, and how to build and sign a Rust app for both in CI. The apps are free, reading-first,
reuse the Rust engine (every crate below `crates/ui-egui` already builds for wasm32), keep user
data in one SQLite User database (ADRs 0006, 0013), read Resources from read-only SQLite `.obr`
files, bundle one Bible and download the rest of the library in-app, sync annotations through a
user-chosen cloud folder (no server of ours), collect no telemetry and have no accounts. Android
ships on Google Play **and** as a signed APK on GitHub Releases.

All sources below were fetched live in this session (late 2026; the newest page dates seen were
App Review Guidelines "Last Updated: October 16, 2026" and the Play Console verification page
"Last updated 2026-09-30"). Each row names the page that owns the rule. Re-check before
submission: every one of these pages changes at least yearly.

## Apple App Store

### Review guidelines that touch our design

| Guideline | Text (quoted or close paraphrase) | What it means for Open Bible |
|---|---|---|
| **2.5.2** | "Apps should be self-contained in their bundles, and may not read or write data outside the designated container area, nor may they download, install, or execute code which introduces or changes features or functionality of the app, including other apps." Only exception: educational apps that teach coding, with source "completely viewable and editable by the user". [Guidelines](https://developer.apple.com/app-store/review/guidelines/) | `.obr` Resources are data (SQLite text + indexes), not code. Downloading them is allowed. Hard line: no downloaded scripts, WASM, SQL triggers/plug-ins or anything that adds features. Keep `.obr` schema strictly declarative. |
| **2.5.1** | Public APIs only; "must run on the currently shipping OS". [Guidelines](https://developer.apple.com/app-store/review/guidelines/) | Fine for Rust static libs; keep Xcode/SDK current (see minimums below). |
| **2.5.15** | "Apps that enable users to view and select files should include items from the Files app and the user's iCloud documents." [Guidelines](https://developer.apple.com/app-store/review/guidelines/) | `.obr` import and the sync-folder picker must use the system document picker (Files + iCloud Drive), not an in-app file browser. |
| **2.5.19** | No "hidden, dormant, or undocumented features". [Guidelines](https://developer.apple.com/app-store/review/guidelines/) | Don't ship desktop-only features dark in the binary without UI. |
| **4.2.3** | "(i) Your app should work on its own without requiring installation of another app to function. (ii) If your app needs to download additional resources in order to function on initial launch, disclose the size of the download and prompt users before doing so." [Guidelines](https://developer.apple.com/app-store/review/guidelines/) | Bundling one Bible means the app works at first launch with no download, so (ii) is not triggered. Library downloads should still show size and need a tap (also good UX on cellular). |
| **4.7** | Only HTML5/JS mini apps, mini games, streaming games, chatbots, plug-ins and emulators may run "software that is not embedded in the binary". [Guidelines](https://developer.apple.com/app-store/review/guidelines/) | Confirms: no downloadable logic of our own. |
| **5.1.1(i)** | "All apps must include a link to their privacy policy in the App Store Connect metadata field and within the app in an easily accessible manner." [Guidelines](https://developer.apple.com/app-store/review/guidelines/) | Need a privacy policy URL and an in-app link even though we collect nothing. |
| **5.1.1(v)** | "If your app doesn't include significant account-based features, let people use it without a login." Account creation ⇒ in-app account deletion. [Guidelines](https://developer.apple.com/app-store/review/guidelines/) | No accounts ⇒ nothing to do. |

### Privacy nutrition label and privacy manifest

| Item | Rule | Source |
|---|---|---|
| Definition of "collect" | "'Collect' refers to transmitting data off the device in a way that allows you and/or your third-party partners to access it for a period longer than what is necessary to service the transmitted request in real time." | [App Privacy Details](https://developer.apple.com/app-store/app-privacy-details/) |
| On-device data | "Data that is processed only on device is not 'collected' and does not need to be disclosed." | same |
| Apple frameworks (e.g. CloudKit) | "You are not responsible for disclosing data collected by Apple" but must disclose what *you* collect via them. | same |
| Privacy policy URL | "Privacy Policy (Required)". | same |
| Required-reason APIs | `stat`, `fstat`, `fstatat`, `lstat` are file-timestamp required-reason APIs; declare in `PrivacyInfo.xcprivacy` with a reason such as `C617.1` (files in app container) or `3B52.1` (files the user granted access to). SDK code must carry its own manifest. | [NSPrivacyAccessedAPIType](https://developer.apple.com/documentation/bundleresources/app-privacy-configuration/nsprivacyaccessedapitypes/nsprivacyaccessedapitype), [Describing use of required reason API](https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api) (page JS-rendered; content taken via search snippet, not full fetch) |

Result: with no telemetry, no crash reporter, and sync going to the user's own iCloud
Drive/Dropbox folder via the document picker, the label is **"Data Not Collected"**. SQLite and
Rust `std::fs` call `stat`/`fstat`, so the app still needs a `PrivacyInfo.xcprivacy` declaring
`NSPrivacyAccessedAPICategoryFileTimestamp` (likely `C617.1` + `3B52.1`). Audit other categories
(system boot time, disk space) against what egui/winit/vello pull in.

### Size, cellular, minimum OS

| Item | Value | Source |
|---|---|---|
| Max uncompressed app (iOS 9+ target) | 4 GB; executable `__TEXT` total 500 MB | [Maximum build file sizes](https://developer.apple.com/help/app-store-connect/reference/app-uploads/maximum-build-file-sizes) |
| Larger content | Background Assets; Apple-hosted asset packs (Managed Background Assets, iOS 26+) up to 200 GB compressed, uploaded to App Store Connect | [Maximum build file sizes](https://developer.apple.com/help/app-store-connect/reference/app-uploads/maximum-build-file-sizes), [Apple-hosted asset packs overview](https://developer.apple.com/help/app-store-connect/manage-asset-packs/overview-of-apple-hosted-asset-packs), [Creating managed asset packs](https://developer.apple.com/documentation/backgroundassets/creating-managed-asset-packs.md) |
| Cellular download | Since iOS 13, Settings › App Store › Cellular Data › App Downloads offers "Always Allow / Ask If Over 200 MB / Ask First"; default asks over 200 MB. Not a hard limit. | Apple support page [Change App Store settings](https://support.apple.com/en-gb/guide/iphone/iph3dfd91de/ios) (429 on fetch; 200 MB figure confirmed only by secondary [MacRumors](https://www.macrumors.com/how-to/download-large-apps-over-cellular-iphone-ipad/)) |
| Minimum SDK | "Since April 28, 2026 … must be built with Xcode 26 or later using an SDK for iOS 26" | [Upcoming Requirements](https://developer.apple.com/news/upcoming-requirements/) |
| Minimum deployment target | "Since September 9, 2026 iOS and iPadOS apps uploaded to App Store Connect must target iOS 13 or later." | same |
| Rust iOS targets | `aarch64-apple-ios`, `aarch64-apple-ios-sim` Tier 2 (no host tools); rustc minimum iOS 10.0, raised via `IPHONEOS_DEPLOYMENT_TARGET` | [rustc book: apple-ios](https://doc.rust-lang.org/nightly/rustc/platform-support/apple-ios.html) |
| Export compliance | Apps whose only encryption is OS-provided HTTPS set `ITSAppUsesNonExemptEncryption = NO` | [ITSAppUsesNonExemptEncryption](https://developer.apple.com/documentation/bundleresources/information-property-list/itsappusesnonexemptencryption) (via search snippet) |
| EU DSA trader status | Apps without verified trader status removed from EU storefronts (since 2025-02-17) | [Upcoming Requirements](https://developer.apple.com/news/upcoming-requirements/) |

### Enrolment and cost

Apple Developer Program: **99 USD per membership year**; fee waiver for nonprofits, accredited
educational institutions, government. Individuals: legal name shown as seller name. Organisations:
legal entity (no DBAs), **D-U-N-S Number**, domain email, live website.
[Apple: Become a member](https://developer.apple.com/programs/enroll/). An individual enrolment
publishes the maintainer's legal name on the store page; an organisation needs a legal entity.

## Google Play

### Policy that touches our design

| Rule | Text | Source |
|---|---|---|
| No self-update, no downloaded code | "An app distributed via Google Play may not modify, replace, or update itself using any method other than Google Play's update mechanism. Likewise, an app may not download executable code (such as dex, JAR, .so files) from a source other than Google Play." Interpreted code in a VM excluded but must not enable violations. | [Device and Network Abuse](https://support.google.com/googleplay/android-developer/answer/9888379?hl=en) |
| File access | All-files access (`MANAGE_EXTERNAL_STORAGE`) restricted to apps whose core purpose needs it; picking individual files should use the Storage Access Framework. | [Use of All files access](https://support.google.com/googleplay/android-developer/answer/10467955), [SAF](https://developer.android.com/training/data-storage/shared/documents-files) |

The **Play build must not contain an in-app updater** (e.g. "new APK on GitHub" check). The
GitHub APK flavour may notify, but if both builds share one binary, gate it at build time.

### Data safety

| Rule | Source |
|---|---|
| Every app on closed/open/production tracks must complete the form; "Even developers with apps that do not collect any user data must complete this form and provide a link to their privacy policy." | [Data safety section](https://support.google.com/googleplay/android-developer/answer/10787469?hl=en) |
| "User data accessed by your app that is only processed locally on the user's device and not sent off device does **not** need to be disclosed." | same |
| FAQ: if "the user chooses to upload their data directly to their own external drive or cloud storage account (such as Google Drive, Dropbox …) … and your app never collects or accesses the data in question, then your app does not need to declare the collection." | same |
| Library/SDK data counts as the app's. | same |

Result: "No data collected, no data shared" is accurate for our design, provided downloads of
Resources send nothing but the HTTP request (ephemeral) and no crash SDK is linked.

### Target API, page size, app size

| Item | Value | Source |
|---|---|---|
| Target API for new apps and updates | **API 36 (Android 16) from 2026-08-31**; extension to 2026-11-01 on request | [Target API level requirements](https://support.google.com/googleplay/android-developer/answer/11926878?hl=en) |
| Existing apps not updated | Must target ≥ API 35 to stay visible to new users on newer Android | same |
| `minSdkVersion` | Independent of target; app can still run "down to the specified `minSdkVersion`" | same |
| 16 KB pages | From 2025-11-01, new apps/updates targeting Android 15+ must support 16 KB page sizes on 64-bit devices; NDK r28+ aligns by default, r27 and lower need `-Wl,-z,max-page-size=16384` | [Support 16 KB page sizes](https://developer.android.com/guide/practices/page-sizes) |
| Size limits (compressed download) | Base module 500 MB; feature module 500 MB; asset pack 1.5 GB; install-time total 4 GB; on-demand/fast-follow 30 GB; "users on a mobile data connection will see a non-blocking dialog" above 200 MB; APK-only uploads limited to 100 MB | [Play size limits](https://support.google.com/googleplay/android-developer/answer/9859372?hl=en) |
| Rust Android targets | Tier 2 (`aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android` …); Rust supports the latest LTS NDK and its API levels | [rustc book: android](https://doc.rust-lang.org/nightly/rustc/platform-support/android.html) |

### Account, testing, verification

| Item | Value | Source |
|---|---|---|
| Registration | US$25 one-time; personal or organisation account; government ID + card in legal name may be required | [Get started with Play Console](https://support.google.com/googleplay/android-developer/answer/6112435?hl=en) |
| New personal accounts (after 2023-11-13) | Closed test with **≥ 12 testers opted in continuously for 14 days** before applying for production | [Testing requirements](https://support.google.com/googleplay/android-developer/answer/14151465?hl=en) |
| Android developer verification | "Android requires all apps to be registered by verified developers for users to install them on certified Android devices." Live **2026-09-30** in Brazil, Indonesia, Singapore, Thailand; "expand … globally for all apps on certified Android devices in 2027." | [Android developer verification](https://developer.android.com/developer-verification/guides) |
| What to do | Verify identity; "Register your package names: Prove ownership of your apps by providing the APK signed with your private key." Play developers are registered automatically; apps on **and** off Play use the Play Console, "where there will be new functionality to register your apps outside of Google Play and keys." | same; [Register on Play Console](https://developer.android.com/developer-verification/guides/google-play-console) |
| Paths for unverified/hobby | Limited distribution (no ID, up to 20 devices); unregistered sideloading via an "advanced flow"; "ADB workflow and experience stays the same." | [Android developer verification](https://developer.android.com/developer-verification/guides) |

## Android signing: Play + GitHub APK

| Fact | Source |
|---|---|
| Play App Signing is mandatory for new apps (optional only for apps created before August 2021); you sign the AAB with an **upload key**, Google signs APKs with the **app signing key**; lost upload key can be reset. | [Sign your app](https://developer.android.com/studio/publish/app-signing) |
| You may let Google generate the signing key, or upload your own ("Export and upload"). | same |
| To use one key across stores: "generate a key locally and upload a copy of it to Play", or use a Google-generated key and "download distribution APKs from Play Console that are signed with the Google-generated key". | [App Bundle FAQ](https://developer.android.com/guide/app-bundle/faq) |
| Updates install only if application ID matches, signing cert matches (or valid proof-of-rotation), version code ≥ installed. Otherwise the user must uninstall, "which erases all app data from the device". | [How app updates work](https://developer.android.com/google/play/app-updates) |
| Same ID + same key ⇒ each store can update the other's installs; different key or ID ⇒ no cross-store updates. Higher version code on the preferred store makes it win. | same |
| Key upgrade through Play applies to Android 13+; older devices keep the old key. | [Sign your app](https://developer.android.com/studio/publish/app-signing) |

Options for the GitHub APK:

| Option | Effect |
|---|---|
| **A. One key (our own, uploaded to Play)** | GitHub APK ↔ Play upgrade freely; user data survives switching. We hold the key in CI secrets ⇒ compromise hits both channels. |
| **B. Separate key, same application ID** | No cross-updates; switching channel = uninstall = **User database wiped** (unless synced). Both keys must be registered for developer verification. |
| **C. Separate ID (e.g. `….openbible.github`)** | Both installable side by side; clean, but two apps, two sync folders to explain. |

## CI for Rust mobile builds

| Piece | Fact | Source |
|---|---|---|
| macOS runners | `macos-latest` / `macos-26` (arm64), `macos-15`; `xcode-27` image also listed | [actions/runner-images](https://github.com/actions/runner-images) |
| Cost | Standard GitHub-hosted runners are free in public repositories; private: macOS $0.062/min vs Linux $0.006/min | [GitHub Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions) |
| Android Rust build | `cargo-ndk` sets NDK env and writes `jniLibs/<abi>/` (e.g. `cargo ndk -t armeabi-v7a -t arm64-v8a -o ./jniLibs build --release`) | [bbqsrc/cargo-ndk](https://github.com/bbqsrc/cargo-ndk) |
| iOS Rust build | `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`; SDK from Xcode via `SDKROOT`/`xcrun`; package static libs into an XCFramework with `xcodebuild -create-xcframework` | [rustc book: apple-ios](https://doc.rust-lang.org/nightly/rustc/platform-support/apple-ios.html); Apple's XCFramework doc is JS-rendered and was not fetched |
| fastlane | `match` (cert/profile sync), `pilot`/`upload_to_testflight`, `deliver` all accept an App Store Connect API key (no 2FA in CI); `supply` uploads AABs to Play | [fastlane: App Store Connect API](https://docs.fastlane.tools/app-store-connect-api/), [supply](https://docs.fastlane.tools/actions/supply/) |

Sketch: Linux job builds Android `.so`s with cargo-ndk (NDK r28+ for 16 KB), Gradle builds AAB
(upload key) + APK (release key), `supply` → Play internal track, APK → GitHub Release. macOS job
builds `aarch64-apple-ios` + simulator static libs, `xcodebuild -create-xcframework`, archive with
Xcode 26, `match` + API key, `pilot` → TestFlight.

## Implications for Open Bible mobile

| Severity | Finding |
|---|---|
| **High** | `.obr` must stay pure data (Apple 2.5.2/4.7, Play Device and Network Abuse). Never ship downloadable logic, scripts or plug-ins in Resources. |
| **High** | GitHub APK signing choice decides whether users can move between GitHub and Play without losing the User database. Option A (one self-held key uploaded to Play) is the only one that preserves data; it needs a key-custody plan. |
| **High** | Android developer verification goes global in 2027: the GitHub APK must be registered (package + key) or users face the "advanced flow". Use the Play Console path for both channels. |
| **High** | Play build must contain no self-update mechanism; gate any GitHub "update available" check out of the Play flavour. |
| Medium | Personal Play account ⇒ 12 testers × 14 days closed test before launch; organisation account skips it but needs D-U-N-S (also for Apple org). Decide account type early. |
| Medium | Targets: Android `targetSdk 36` now (and yearly bumps each August 31), NDK r28+; iOS built with Xcode 26 / iOS 26 SDK, deployment target ≥ iOS 13 (pick realistic higher floor for UI stack). |
| Medium | Privacy: label "Data Not Collected" and Data safety "no data collected/shared" are accurate, but both stores still need a privacy policy URL and in-app link, and iOS needs `PrivacyInfo.xcprivacy` for file-timestamp APIs used by SQLite/std. |
| Medium | File import and sync-folder choice must use the system document picker (iOS 2.5.15) / Storage Access Framework (avoid `MANAGE_EXTERNAL_STORAGE`). |
| Low | Bundling one Bible avoids 4.2.3(ii); keep the base app under 200 MB to avoid cellular prompts on both stores. Optional: Apple-hosted asset packs / Play Asset Delivery for library, but our own static host keeps one path for both stores and the APK. |
| Low | CI is free on the public repo's standard runners; macOS needed only for the iOS job. |

## Open questions for the decision ticket

1. Publisher identity: individual (legal name shown, 12-tester rule on Play) or an organisation (D-U-N-S, website, possible Apple fee waiver if nonprofit)?
2. GitHub APK: same key as Play (Option A), separate key, or separate application ID? Who holds the key and where (CI secret, HSM)?
3. Minimum iOS and Android versions, given the UI stack (egui/winit vs native) still open.
4. Where are library Resources hosted (GitHub Releases, own static host, Apple/Play asset packs)? Must be the same for the GitHub APK.
5. Does sync use only the system document picker (iCloud Drive/Files, SAF), or a provider SDK (Dropbox/Google Drive)? An SDK would change the privacy answers.
6. F-Droid as a third channel (reproducible build, its own key) — in or out of scope?

## Gaps

- Apple's 200 MB cellular default is from Apple's settings page (fetch returned 429) and a secondary source; behaviour of the prompt not checked against current iOS 26.
- Apple's required-reason API and XCFramework pages are JavaScript-rendered; content taken from search snippets.
- Developer verification details for the off-Play key registration flow in Play Console ("there will be new functionality") not yet documented in detail.
- The 16 KB rule's Play enforcement page was read via the Android developers guide and search snippet, not the Play Console help page.
