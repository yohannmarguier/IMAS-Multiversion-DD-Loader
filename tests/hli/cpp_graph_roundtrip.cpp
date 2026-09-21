// The graph supports beta_normal -> beta_tor_norm and a sign flip on
// profiles_1d/psi. The legacy HLI round-trip instead requires the XML-only
// psi_axis split; it remains covered by the complete private-fixture suite.
// Run this against separate copies of the old-DD pulse and same-DD control.
#include "ALClasses.h"

#include <cmath>
#include <cstdio>
#include <string>

#define CHECK(condition)                                                        \
  do {                                                                          \
    if (!(condition)) {                                                         \
      std::fprintf(stderr, "GRAPH-ROUNDTRIP-FAILURE at line %d: %s\n", __LINE__,   \
                   #condition);                                                \
      return 1;                                                                 \
    }                                                                           \
  } while (0)

int main(int argc, char* argv[]) {
  CHECK(argc == 3);
  const std::string mode = argv[2];
  CHECK(mode == "cross" || mode == "same");
  const bool cross = mode == "cross";
  const std::string uri = std::string("imas:hdf5?path=") + argv[1];
  auto readable = [cross](int status) {
    return status == 0 || (cross && status == IdsNs::PARTIAL_READ);
  };

  IdsNs::IDS before;
  CHECK(before.open(uri, OPEN_PULSE) == 0);
  CHECK(readable(before._equilibrium.get()));
  const int slices = before._equilibrium.time_slice.extent(0);
  const int times = before._equilibrium.time.extent(0);
  const int timeMode = before._equilibrium.ids_properties.homogeneous_time;
  CHECK(slices > 0 && slices == times);
  // Under the old stamp this value exists only at beta_normal, so this
  // assertion fails if the HLI bypasses conversion or reads an empty scalar.
  CHECK(std::fabs(before._equilibrium.time_slice(0).global_quantities.beta_tor_norm -
                  1.8) < 1e-12);
  before.close();

  IdsNs::IDS append;
  CHECK(append.open(uri, OPEN_PULSE) == 0);
  append._equilibrium.ids_properties.homogeneous_time = timeMode;
  append._equilibrium.time.resize(1);
  append._equilibrium.time(0) = 2.0;
  append._equilibrium.time_slice.resize(1);
  auto& slice = append._equilibrium.time_slice(0);
  slice.time = 2.0;
  slice.global_quantities.beta_tor_norm = 7.25;
  slice.profiles_1d.psi.resize(2);
  slice.profiles_1d.psi(0) = 7.5;
  slice.profiles_1d.psi(1) = -3.25;
  const int putStatus = append._equilibrium.putSlice();
  CHECK(putStatus == 0 || (cross && putStatus == IdsNs::PARTIAL_PUT));
  CHECK(slice.profiles_1d.psi(0) == 7.5 && slice.profiles_1d.psi(1) == -3.25);
  append.close();

  IdsNs::IDS after;
  CHECK(after.open(uri, OPEN_PULSE) == 0);
  CHECK(readable(after._equilibrium.get()));
  CHECK(after._equilibrium.time_slice.extent(0) == slices + 1);
  CHECK(after._equilibrium.time.extent(0) == times + 1);
  CHECK(after._equilibrium.ids_properties.homogeneous_time == timeMode);
  CHECK(after._equilibrium.time(times) == 2.0);
  const auto& result = after._equilibrium.time_slice(slices);
  CHECK(result.time == 2.0);
  CHECK(result.global_quantities.beta_tor_norm == 7.25);
  CHECK(result.profiles_1d.psi.extent(0) == 2);
  CHECK(result.profiles_1d.psi(0) == 7.5 && result.profiles_1d.psi(1) == -3.25);
  after.close();
  std::printf("graph-supported C++ %s-DD read/write round trip passed\n", argv[2]);
  return 0;
}
