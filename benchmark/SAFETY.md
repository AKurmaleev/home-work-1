# Existing native benchmark integration

## Narrow low-level exception

The homework libraries forbid unsafe code. The existing optional native benchmark
has a separate low-level integration boundary under Rust Rule 10, restricted to
`src/windows_native.rs` on `x86_64-pc-windows-gnu`. This exception covers only the
retained Win32 process-memory and thread-affinity calls and their ABI declarations;
it does not authorize unsafe application logic, new FFI, assembly, allocators,
or hardware-specific optimization.

The boundary uses the Windows x64 `system` ABI. Process and thread pseudo-handles
are borrowed for a call, never closed or retained as owned handles. Writable output
pointers address initialized local storage for the call's duration. Compile-time
checks cover the size, alignment, and all offsets of `PROCESS_MEMORY_COUNTERS`.
The affinity guard is neither `Send` nor `Sync`, so restoration is attempted on
the originating thread. Win32 failure returns are checked; destructor restoration
failures are diagnosed rather than panicking. Peak Working Set is process-wide,
not heap allocation, committed memory, or an allocation count.

## Process ownership and failure policy

Worker executables must be trusted benchmark builds. They must implement the
bounded one-record-per-command protocol and must not spawn descendants that retain
pipe handles. Paths are passed directly to `Command`, never interpreted by a shell.
The runner inherits the operator's working directory and environment. This is a
local measurement harness, not a sandbox or an untrusted-process supervisor.

A child guard takes ownership immediately after spawn, including partial setup.
Explicit completion attempts both workers and both pipe-reader joins even when an
earlier cleanup step fails. Killing and bounded exit polling are separate attempts:
a failed kill never skips waiting. The initiating error keeps precedence and later
failures are appended to its diagnostics. Timeouts remain failures even if cleanup
subsequently succeeds. No successful result promises cleanup when exit is unverified.

Cancellation terminates before closing a potentially blocked command pipe. Exit
polling uses a 120-second operation deadline and five-second termination/reap
windows. If termination cannot be verified, cleanup reports that fact and does not
block indefinitely joining a reader. Destructor fallback cannot return errors;
it reports them to stderr without panicking. Failure of that last reporting channel
may lose the diagnostic, an explicitly tested exception, not successful cleanup.
Aborts and process termination may bypass destructors entirely.

## Verification and limits

`cargo test --locked --workspace` includes native Win32 layout/memory/affinity
smoke coverage on the GNU Windows target. Deterministic seams test kill/wait
failure combinations, paired worker completion, pipe-reader errors, reader panic,
failed affinity restoration, and unavailable stderr. A real owned-child test
checks timeout termination and reaping without running a benchmark workload.

These checks do not establish every OS failure, native thread-spawn failure,
descendant-process cleanup, or recovery from a failed Win32 affinity restore.
No Miri, sanitizer, alternate target, or new hardware benchmark campaign was run.
Portable/specialized build labels and codegen parsing have separate integration
tests. Specialized executables retain their documented host/ISA preconditions;
this review adds no tuning and makes no new performance or portability claim.
