# AriaFlow Browser Extensions

The extension sends HTTP(S) download URLs directly to `ariaflowd` through its
HTTP Web API. Native Messaging and `ariaflow-native-host` are not required.

## Install and configure

1. Start `ariaflowd` with its Web API enabled (default: `http://127.0.0.1:8787`).
2. Chromium: open `chrome://extensions`, enable Developer mode, choose **Load
   unpacked**, and select `browser-extension/chromium/`.
3. Firefox: open `about:debugging#/runtime/this-firefox`, choose **Load Temporary
   Add-on**, and select `browser-extension/firefox/manifest.json`. A permanent
   installation requires a signed add-on.
4. Open the extension's options from its extension-management page. Set the API
   URL and, if enabled on the daemon, the matching bearer token. Test and save.
   These settings are independent from the AriaFlow desktop/web settings.

Automatic interception can be disabled in options; the link context menu remains
available. The extension requires access to URLs to contact the configured API
and uses browser-local extension storage for its configuration. Remote APIs must
use HTTPS; protect the token and do not store it on untrusted devices. For remote
access enable daemon authentication and secure network access.

The extension creates an API task before cancelling the browser download. If the
API returns an error, the browser download continues. An ambiguous timeout or
lost response may nevertheless have created a daemon task; check for duplicates
before retrying. If the browser finishes before cancellation, both copies may
remain. Right-click failures are shown as notifications. Only HTTP(S) URLs are
transferred; website cookies and request bodies are not forwarded, so some
signed, authenticated, POST, or session-bound downloads may not work in AriaFlow.

The previous native host remains available as a legacy component for existing
integrations, but the extensions no longer depend on it or its host manifests.
