# macOS screen capture permissions

Researched 2026-09-29 against Apple documentation and Apple Developer Technical Support responses.

## Appearance and authorization

Signing an app does not request Screen Recording access. The bundled app must exercise a permission request or capture flow. Apple documents `CGPreflightScreenCaptureAccess()` as a check without a prompt and `CGRequestScreenCaptureAccess()` as the request API. Apple's ScreenCaptureKit sample prompts on its initial run and directs the user to System Settings to grant Screen Recording access. [Apple DTS: supported authorization APIs](https://developer.apple.com/forums/thread/839069?answerId=898801022), [Apple capture sample](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos).

For Quest Display, a user initiated permission button in the bundled GUI can request access before connecting a browser. That request is the expected route to establish the app's entry in Screen Recording / Screen & System Audio Recording settings. This is an implementation recommendation; the cited APIs do not promise the precise timing or wording of the Settings row on every macOS version. Merely opening the GUI or adding an Info.plist purpose string should not be described as a permission grant. Apple calls for `NSScreenCaptureUsageDescription` to explain the access. [ScreenCaptureKit overview](https://developer.apple.com/documentation/screencapturekit).

The system content-sharing picker is a separate path: on macOS Sonoma and later, `SCContentSharingPicker` can authorize capture of the user's selected content for the session without separate full-screen permission. Do not mistake picker session access for a persistent grant in Settings. [Apple WWDC23: What's new in privacy, screen capture picker](https://developer.apple.com/videos/play/wwdc2023/10053/).

## Permission persistence and signing

Apple's TN3127 explains that macOS identifies code through its designated requirement. An ad-hoc signature's requirement identifies a particular build, so keeping the bundle identifier alone does not make permissions reliably survive code changes. Apple DTS explicitly confirms this behavior for ScreenCaptureKit rebuilds and points to TN3127. Use an Apple Development signing identity consistently for local development. A switch from development signing to Developer ID signing can require a fresh grant because the default requirements differ. [TN3127: Inside Code Signing: Requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements), [Apple DTS: ScreenCaptureKit permissions lost after every build](https://developer.apple.com/forums/thread/819406).

Developer ID Application signing and notarization are the distribution workflow for apps delivered outside the Mac App Store. These support Gatekeeper checks; the person still controls access to privacy protected screen content. They should not be presented as necessary steps merely to trigger a local permission request. [Apple: Signing your apps for Gatekeeper](https://developer.apple.com/developer-id/), [Apple: Notarizing macOS software before distribution](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).

## Repository findings and recommended change

At inspection, `packaging/macos/Info.plist` already supplies the stable bundle identifier `io.github.gitfudge0.questdisplay`, the display name `Quest Display`, and `NSScreenCaptureUsageDescription`. `scripts/package-macos-app.sh` signs with `CODE_SIGN_IDENTITY`, defaulting to ad-hoc `-`. `make run` launches the app bundle with `start`, while `make gui` and `make gui-dev` launch the Cargo executable directly. Capture starts later, during browser negotiation.

Launch the GPUI dashboard from the same packaged app, expose a user initiated authorization request, and support a selected Apple Development certificate for local builds. Retain ad-hoc builds as a local fallback while explaining that changed builds can need another grant. Verify actual Settings appearance and capture on the target Mac; source research does not establish that the existing app is already listed or authorized.
