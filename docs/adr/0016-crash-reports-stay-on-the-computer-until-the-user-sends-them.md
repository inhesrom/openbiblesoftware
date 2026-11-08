# Crash reports stay on the computer until the user sends them

A desktop app that crashes leaves the user with nothing to tell us, and an offline-first app must not phone home (#219). We decided:

1. **A panic hook writes a Crash report to the Log folder**, installed by the desktop app before anything else runs: of the panic message only a standard library phrase it starts with (such as ``called `Option::unwrap()` on a `None` value`` or `index out of bounds`), then its source location, the thread, a forced backtrace, the app version, the operating system and the last log lines, with the home folder written as `~`. Nothing from the User database or the library is read for it. The hook then runs the previous one, so the panic still prints and the process ends as before.
2. **The app logs through the `log` facade to its own rotating file.** Libraries emit `log` records (command ids, job names, failures, never parameters or text; a job's name is fixed text, `&'static str`, so it cannot carry a Resource id made from an imported file's name); the app installs the one logger, which writes `openbible.log` in the Log folder (another library's message is written only as the library's source file and line it was logged from, unless the library is a vetted graphics crate: egui-winit, for one, logs a whole text-input event, typed text included), rotates it at 1 MB keeping three old files, and keeps the last lines in memory for reports. The Log level is a preference in the User database. Other crates' records are written from Warn up unless the level is Detailed.
3. **Sending is always the user's act.** The next launch offers Report a problem for a new Crash report, once; the window shows the whole text and says, before either button is clicked, what each does. Only Copy report (the clipboard) or Open GitHub issue… take it anywhere. Open GitHub issue… puts the report in the new-issue form's web address, so opening it sends the report to GitHub; it becomes a public issue only if the user submits the form. The engine has no network code for it.

## Considered Options

- **Automatic upload (a crash-reporting service):** needs a network client and a server, sends without consent, and would carry paths and messages we cannot vet in advance.
- **A crash-handler process or minidumps:** catches native crashes too, but needs a second binary and platform tooling; a Rust panic is what we can catch and explain, and the backtrace is enough to find it.
- **Save the Last session from the panic hook:** the hook cannot reach the Session safely (it may be mid-change on another thread). Instead the Last session is saved within a second of every change: a skipped save asks for the frame that makes it.

## Consequences

- A panic on a worker thread that the job queue catches still writes a Crash report: it is a bug worth reporting even though the app carried on.
- A panic message can carry data the code put in it, quoted or not (an `expect` message, an error's fields, a string it could not slice), so the hook keeps none of it: a message that starts with one of a fixed list of the standard library's phrases keeps that phrase and is written `(details withheld)` after it; any other is written `(message withheld)`. The same goes for the log file, which gets the panic line too. The source location and the backtrace still say where it happened; the indices and values that would say why are lost, and are found by reproducing it. The user still reads every report before it leaves, and the home folder is redacted.
- A library's log message is lost for diagnosis unless that library is vetted; its source file and line still say what it was.
- Crash reports are named by creating the file (`create_new`), so threads panicking in the same second each keep their own; a report's number is above every report of its second still in the folder, so keeping the newest ten never deletes the one just written.
- Native crashes (a segfault in a graphics driver) write no report; the log file still shows what led up to them.
- The forced panic (`OBS_FORCE_PANIC`) exists only in debug builds, for the end-to-end test.
