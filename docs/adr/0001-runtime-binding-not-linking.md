# Runtime binding to IMAS-Core instead of linking against it

The shim re-exports IMAS-Core's public C ABI verbatim, so it both *defines* and *calls* symbols like `al_read_data`. A conventional link would resolve the shim's own outbound call straight back to its own definition: the OS loader's rule is that the first definition encountered in load order wins, and the shim is loaded ahead of IMAS-Core, so the result is unbounded recursion rather than a call into IMAS-Core. We decided the shim carries no link-time dependency on IMAS-Core at all: it opens IMAS-Core at runtime into a private symbol scope and calls through a function-pointer table, so its own exported definitions are never in a position to capture its outbound calls.

## Considered Options

- **Symbol renaming** — export the mirrored functions under different names and rename them back at the HLI boundary. Rejected: breaks the project's core premise of verbatim ABI re-export, and pushes the problem onto every consumer.
- **Preload interposition** (`LD_PRELOAD`-style) — let the shim intercept calls made by an unmodified binary. Rejected: fragile, platform-specific, and invisible in a normal build's link line — nothing in an HLI's build would show that interposition is happening.
- **Abandoning verbatim mirroring** — give the shim its own distinct API and require every HLI to adapt to it. Rejected: multiplies the integration cost across five HLIs instead of concentrating it once, in the shim.
- **Runtime binding via the platform's dynamic-loading API, into a private scope** — chosen. It preserves verbatim mirroring, requires no HLI-side API change, and the private scope is exactly what prevents the recursion.

## Consequences

- No linker ever validates that the shim's hand-written signatures agree with IMAS-Core's real ones. This is why a separate CI check compiles a translation unit against IMAS-Core's real header and asserts struct layout and function addresses — the check a normal link would have given for free.
- Because there is no link-time record of which IMAS-Core the shim was built against, a runtime version check is required: the shim must ask IMAS-Core its version on first resolution and compare it against the version it was built against, so a mismatch fails through the ABI's status channel instead of calling through the wrong signatures.
