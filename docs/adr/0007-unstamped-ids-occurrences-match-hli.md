# Unstamped IDS occurrences are presumed to match the HLI DD version

When the HLI DD version has been supplied and an IDS occurrence has no DD-version stamp at `ids_properties/version_put/data_dictionary`, the shim presumes the occurrence uses that HLI DD version. It creates no context-registry record and forwards all paths unchanged. A version-mismatched context is registered only when a present, valid DD-version stamp identifies a stored DD version different from the HLI DD version.

The shim makes no second read to distinguish an unstamped occurrence from an absent occurrence, does not infer a version from filled paths, and offers no caller-supplied stored-version override. Those mechanisms either add a per-open cost or work only for selected backends, while an override would create a competing source of truth for an occurrence. No conversion occurs on this path, so the shim emits no conversion-loss report; an unstamped occurrence is explicitly part of the identity-forwarding contract, not a failed conversion.

## Considered Options

- **Refuse unstamped occurrences** — rejected. `put_slice` writes dynamic data without the constant DD-version stamp, making valid unstamped occurrences a normal case rather than an exceptional corruption path.
- **Assume a fixed default stored DD version** — rejected. A process-wide default cannot describe different occurrences and would fabricate a mismatch without occurrence metadata.
- **Infer the version from stored paths** — rejected. `al_list_filled_paths` is unavailable on most IMAS-Core backends and therefore cannot define the shim's general read-path behaviour.
- **Let the caller declare an unstamped occurrence's stored version** — rejected. An absent stamp means identity forwarding by contract; a second declaration route would make the stored-version source ambiguous.
