# SpektraFilm 资产部署（PhotoCraft-Spektrafilm · `custom-combined` 分支）

本分支通过源码级集成（路线 D）把 [spektrafilm](https://github.com/andreavolpato/spektrafilm) / [spektrafilm-rust](https://github.com/turbasvin/spektrafilm-rust) 的物理胶片模拟（`spektra-core`，GPL-3.0-or-later）与 [SpektraLab](https://github.com/JamesQiu2005/SpektraLab) 风格三栏暗房工作台接入 PhotoCraft，提供 `filter.spektra.filmSim` 滤镜（GUI：`滤镜 › SpektraFilm › 胶片模拟…` 及顶部/属性栏快捷入口；CLI：`photocraft-cli run`）。完整暗房工作台与扩展物理参数说明见 [`USAGE.zh-CN.md`](USAGE.zh-CN.md)。

## 资产布局

源码树内为 `assets/spektra/`（约 30 MB）：

```
spektra/
└── oracle/
    ├── *.bin          # cmfs / illuminants / spectra LUT / matrices / raw LUT
    ├── filters/       # 滤镜静态数据
    ├── golden_*.json  # parity 测试夹具（运行时不需要，随包部署无害）
    └── profiles/      # 28 个 profile JSON（film stock 定义）
```

代码也接受根级 `spektra/profiles/`，但部署以 `oracle/profiles/` 为准；`film` 取该目录下 JSON 文件名（去扩展名），如 `kodak_portra_400`、`fujifilm_velvia_100`。

## 资产解析顺序（crates/spektra-core/src/assets.rs）

命中即停，共三级：

1. 环境变量 `PHOTOCRAFT_SPEKTTRA_ASSETS`（指向 `spektra` 根目录，须含 `oracle/`）
2. `<可执行文件目录>/spektra/`
3. 当前工作目录下的 `./spektra`（按 CWD 解析）

注意：env 一旦设置即优先，不是兜底——改本地目录前请先取消 env；env 名拼写为 `SPEKTTRA`（双 T，项目名 spektra 为单 t，勿混）。找不到资产时滤镜报错，按上述顺序排查。

## 部署

将 `assets/spektra/` 整个目录复制到可执行文件同级（GUI 与 CLI 同目录时一份即可）：

```
<PhotoCraft 目录>/
├── photocraft.exe
├── photocraft-cli(.exe)
└── spektra/              ← 复制自仓库 assets/spektra/
    └── oracle/...
```

校验：`spektra/` 包 SHA-256 见发布页；资产与代码版本错配会导致错误结果（见"版本对应"）。

## CLI 用法
```bash
photocraft-cli run in.png --cmd filter.spektra.filmSim --params "{\"film\":\"kodak_portra_400\",\"ev\":0,\"printChain\":true}" --out out.png
```

| 参数 | 说明 | 默认 |
|---|---|---|
| `film` | `oracle/profiles/` 下 JSON 文件名去扩展名（22 种胶片） | `kodak_portra_400` |
| `paper` | 相纸 stock（6 种相纸），仅负片/电影片且 `printChain` 开启时使用（正片自动透射直扫） | `kodak_portra_endura` |
| `ev` | 相机曝光补偿 -4..4（线性输入按 2^ev 缩放） | `0` |
| `printChain` | 完整暗房相纸印相链（负片→放大机→相纸→扫描）；`false` 仅输出负片直扫（研究用途） | `true` |
| 扩展参数 | `couplers` / `couplersAmount` / `inputGamut` / `outputGamut` / `printEv` / `ccCyan` / `ccMagenta` / `ccYellow` | 关 / `0` |

Windows cmd 不支持单引号，JSON 内双引号需转义（`\"`）；PowerShell 可用单引号包裹整个 JSON。
仅支持 RGB 图层；智能对象上应用会记录为可重渲染的智能滤镜。建议把 `spektra/` 放在 CLI 二进制旁，或在任意目录运行前设置 env（第 3 级按 CWD 解析）。

## 版本与许可证

- 本组合源码及构建产物整体适用 **GPL-3.0-or-later**（见仓库根目录 [`LICENSE-GPLv3`](../../LICENSE-GPLv3) 与 [`NOTICE`](../../NOTICE)）；胶片/相纸测量数据文件采用 CC BY-SA 4.0（详见 [`ATTRIBUTION.md`](../../ATTRIBUTION.md)）。
- 上游致谢：`storytold/photocraft`（PhotoCraft 基座）、`andreavolpato/spektrafilm`（Python 物理建模母本）、`turbasvin/spektrafilm-rust`（Rust `spektra-core` 移植）、`JamesQiu2005/SpektraLab`（三栏暗房界面与胶片盒封面设计参考）。
- 若需保留纯 MIT/Apache 许可，请移除 `crates/spektra-core/` 并改用独立进程通信（路线 B）。
