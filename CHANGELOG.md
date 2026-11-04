# Changelog

## [0.2.0](https://github.com/breuerpeter/ulog-rrd/compare/v0.1.0...v0.2.0) (2026-11-04)


### ⚠ BREAKING CHANGES

* the rotation and translation options now place PX4's north-east-down frame, not an east-north-up one; a z-up world with x north takes a half turn about x, (1, 0, 0, 0).

### Features

* rows keep PX4's frames, and one transform at the root places them ([0f5c3f6](https://github.com/breuerpeter/ulog-rrd/commit/0f5c3f6092327bd9c7bc23b135085c859f7dfdc1))


### Bug Fixes

* the body axes point where PX4 points them ([6313754](https://github.com/breuerpeter/ulog-rrd/commit/63137540179de74b2723955e792a350940ae3392))

## 0.1.0 (2026-11-03)


### Features

* one ULog to .rrd converter for pulsar and nexus ([ac7fa68](https://github.com/breuerpeter/ulog-rrd/commit/ac7fa68a14127602f7a748511c2bdb466ccc061c))
* the npm package, wasm for the browser ([8c94787](https://github.com/breuerpeter/ulog-rrd/commit/8c947873bd04002d29151949641eadeb97a04e11))
* the Python package, a PyO3 wheel ([d010374](https://github.com/breuerpeter/ulog-rrd/commit/d0103745a1a641a49c29af367d13d525e461a28c))
* the ULog to .rrd converter as a Rust crate ([98cfc99](https://github.com/breuerpeter/ulog-rrd/commit/98cfc99beb7cebd7456136cef25809cfb27815af))


### Bug Fixes

* append writes the recording anew under one footer ([7fcb8ba](https://github.com/breuerpeter/ulog-rrd/commit/7fcb8badc71e757e70d348788d4720da4dcb4178))

## Changelog
