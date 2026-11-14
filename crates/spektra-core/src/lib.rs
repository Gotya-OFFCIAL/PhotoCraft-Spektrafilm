//! spektra-core — 光谱模拟核心原语
//!
//! 母本项目为 [spektrafilm](https://github.com/andreavolpato/spektrafilm)（GPL-3.0）。
//! M1：负片直扫（scan_film）确定性逐像素路径，与上游 oracle 黄金向量对拍。
//!
//! 许可证：GPL-3.0-or-later。

pub mod assets;
pub mod cam16;
pub mod color;
pub mod couplers;
pub mod film;
pub mod gamut;
pub mod grain;
pub mod grid;
pub mod pipeline;
pub mod printing;
pub mod profile;
pub mod spectral;
pub mod types;
pub mod upsampling;

pub use assets::{oracle_dir, profiles_dir};
pub use grid::{WL_COUNT, WL_START_NM, WL_STEP_NM};
pub use spectral::SpectralDistribution;
pub use types::{FilmProfile, ProfileData, ProfileInfo, RawLut, SpectraLut, Tables};

// oracle 资产目录改为运行时解析：见 assets 模块的 oracle_dir()/profiles_dir()。

