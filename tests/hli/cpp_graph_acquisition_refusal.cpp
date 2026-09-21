// Drives one uncached mismatch through the public C++ HLI while the workflow
// deliberately withholds graph credentials. The program checks the status and
// untouched IDS; the workflow separately checks the HLI-emitted refusal text,
// because the generated get() API returns the code but not the message.
#include "ALClasses.h"

#include <cstdio>
#include <string>

namespace {

constexpr const char* kFailureMarker = "GRAPH-ACQUISITION-REFUSAL-FAILURE";

bool isRefusal(int status) {
  return status >= IdsNs::AL_REFUSAL_BAND_MIN && status <= IdsNs::AL_REFUSAL_BAND_MAX;
}

void expect(bool condition, const char* message, int& failures) {
  if (!condition) {
    std::printf("%s: %s\n", kFailureMarker, message);
    ++failures;
  }
}

}  // namespace

int main(int argc, char* argv[]) {
  if (argc != 2) {
    std::printf("%s: missing unsupported-version fixture\n", kFailureMarker);
    return 1;
  }

  int failures = 0;
  IdsNs::IDS ids;
  const std::string uri = std::string("imas:hdf5?path=") + argv[1];
  expect(ids.open(uri, OPEN_PULSE) == 0, "data-entry open did not forward", failures);
  expect(ids.isConnected(), "data-entry open returned no usable handle", failures);

  const int getStatus = ids._equilibrium.get();
  ids.close();

  expect(isRefusal(getStatus), "uncached unavailable graph did not refuse", failures);
  expect(!ids._equilibrium.isDefined(), "refused acquisition defined the IDS", failures);
  expect(ids._equilibrium.time.size() == 0, "refused acquisition populated IDS data", failures);

  if (failures != 0) {
    std::printf("%s: %d expectation(s) failed\n", kFailureMarker, failures);
    return 1;
  }
  std::printf("graph acquisition refusal reached the C++ HLI status channel\n");
  return 0;
}
