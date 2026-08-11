# Fidelity verdicts distinguish potential from certain loss

The shim retains four fidelity verdicts: exact, potentially lossy and unverified, certainly lossy, and unmappable. A conditional rule establishes only potential loss; normal reads do not perform auxiliary reads or floating-point comparisons to verify it, because that would add hidden read cost and require an unjustified tolerance policy. How these verdicts reach the caller through `al_status_t` remains a separate reporting-channel decision.
