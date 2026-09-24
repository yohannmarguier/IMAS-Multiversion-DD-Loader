/* A version-compatible but incomplete IMAS-Core stand-in.
 *
 * Runtime binding must accept getALVersion as its bootstrap probe, then refuse
 * the entire library when its first required manifest symbol is absent.  Keep
 * this fixture deliberately tiny: exporting al_context_info would turn the
 * missing-symbol scenario into a successful forwarding scenario instead. */

#ifndef MISSING_SYMBOL_STUB_VERSION
#error "MISSING_SYMBOL_STUB_VERSION must be set by CMakeLists.txt"
#endif

const char *getALVersion(void) {
    return MISSING_SYMBOL_STUB_VERSION;
}
