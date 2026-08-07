/* A fake IMAS-Core for tests/tracer_bullet_context_info.c: it exports
 * `al_context_info` and `getALVersion` with IMAS-Core's real signatures (see
 * tests/support/recording_stub.c), and these extra accessors so a test can
 * see what it received without any shared state beyond this one process. */

#ifndef IMAS_MVDD_RECORDING_STUB_H
#define IMAS_MVDD_RECORDING_STUB_H

#ifdef __cplusplus
extern "C" {
#endif

int recording_stub_call_count(void);
int recording_stub_last_ctx(void);
int recording_stub_version_query_count(void);
void recording_stub_set_al_version(const char *version);
void recording_stub_reset(void);

#ifdef __cplusplus
}
#endif

#endif /* IMAS_MVDD_RECORDING_STUB_H */
