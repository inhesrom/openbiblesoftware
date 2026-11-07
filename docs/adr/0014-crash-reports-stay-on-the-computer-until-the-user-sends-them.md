# Crash reports stay on the computer until the user sends them

A desktop app that crashes leaves the user with nothing to tell us, and an offline-first app must not phone home (#219). We decided:

1. **A panic hook writes a Crash report to the Log folder**, installed by the desktop app before anything else runs: the panic message and source location, the thread, a forced backtrace, the app version, the operating system and the last log lines, with the home folder written as `~`. Nothing from the User database or the library is read for it. The hook then runs the previous one, so the panic still prints and the process ends as before.
2. **The app logs through the `log` facade to its own rotating file.** Libraries emit `log` records (command ids, job names, failures, never parameters or text); the app installs the one logger, which writes `openbible.log` in the Log folder, rotates it at 1 MB keeping three old files, and keeps the last lines in memory for reports. The Log level is a preference in the User database. Other crates' records are written from Warn up unless the level is Detailed.
3. **Sending is always the user's act.** The next launch offers Report a problem for a new Crash report, once; the window shows the whole text. Only Copy report (the clipboard) or Open GitHub issue… (the browser, with the report in the new-issue form, submitted by the user there) take it anywhere. The engine has no network code for it.

## Considered Options

- **Automatic upload (a crash-reporting service):** needs a network client and a server, sends without consent, and would carry paths and messages we cannot vet in advance.
- **A crash-handler process or minidumps:** catches native crashes too, but needs a second binary and platform tooling; a Rust panic is what we can catch and explain, and the backtrace is enough to find it.
- **Save the Last session from the panic hook:** the hook cannot reach the Session safely (it may be mid-change on another thread). Instead the Last session is saved within a second of every change: a skipped save asks for the frame that makes it.

## Consequences

- A panic on a worker thread that the job queue catches still writes a Crash report: it is a bug worth reporting even though the app carried on.
- A panic message can quote data the code put in it; the user reads every report before it leaves, and the home folder is redacted.
- Native crashes (a segfault in a graphics driver) write no report; the log file still shows what led up to them.
- The forced panic (`OBS_FORCE_PANIC`) exists only in debug builds, for the end-to-end test.
