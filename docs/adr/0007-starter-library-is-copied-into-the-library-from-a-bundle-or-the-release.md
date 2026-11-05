# The Starter library is copied into the Library, from a bundle or from the release

The Starter library reaches the user in one of two ways: an installer **bundles** `dist/starter/` (found at `$OBS_STARTER_DIR`, `starter/` beside the executable, or the macOS bundle's `Resources/starter/`), or the app **downloads** it from the release assets (`<id>.obr` and `starter-manifest.json`). Either way the app copies each Starter `.obr` into the user's library folder, after checking its sha256 against `starter-manifest.json`, and adds it through the same Library entry path as Import. Which Starter Resources the user removed is kept in the User database, so a removal is never undone silently. A Starter `.obr` with the same `resource_id` and a higher `revision` replaces the copy in the library folder as a Resource revision.

## Considered Options

- **Read the Starter library in place from the bundle:** no copy (about 180 MB), but Remove could not delete a file inside a signed, read-only app bundle, and replacing the app would swap Resources under the user's Annotations with no re-anchoring step.
- **Copy into the library folder (chosen):** one copy on first run. Every Resource then lives in one folder and behaves alike: Remove, the Preferred order, Search index caches and re-anchoring work as they do for imported Resources.
- **Download only, no bundle:** smallest installer, but the app would be empty offline on first launch, against the Starter library's definition ("available offline from first launch").

## Consequences

- Releases must keep publishing `starter-manifest.json` and one `<resource_id>.obr` per Starter Resource under `releases/latest/download/`; the manifest's `file_sha256` is what a download is checked against. Installers must bundle the whole of `dist/starter/` in one of the places the app looks.
- `resource_id` stays fixed once shipped (#20): it is how a later bundle or release recognizes a Resource as a revision rather than a new one.
- The User database gains the `starter-library` preference (which Starter Resources were added, which were removed).
