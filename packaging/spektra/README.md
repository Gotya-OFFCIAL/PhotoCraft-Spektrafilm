# SpektraFilm 资产部署（PhotoCraft fork · spektra-film-integration 分支）

本分支通过源码级集成（路线 D）把 [spektrafilm](https://github.com/andreavolpato/spektrafilm) 的物理胶片模拟
（spektra-core，GPL-3.0-or-later）接入 PhotoCraft，提供 `filter.spektra.filmSim` 滤镜（GUI：Filter › SpektraFilm › Film Simulation…；CLI：`photocraft-cli run`）。

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
```
photocraft-cli run in.png --cmd filter.spektra.filmSim --params "{\"film\":\"kodak_portra_400\",\"ev\":0,\"printChain\":false}" --out out.png
```

| 参数 | 说明 | 默认 |
|---|---|---|
| `film` | `oracle/profiles/` 下 JSON 文件名去扩展名 | `kodak_portra_400` |
| `paper` | 相纸 stock，仅 `printChain` 开启时使用 | `kodak_portra_endura` |
| `ev` | 曝光补偿 -4..4（线性输入按 2^ev 缩放） | `0` |
| `printChain` | 完整 print 链（负片→相纸→扫描）；关闭则仅负片直扫 | `false` |

Windows cmd 不支持单引号，JSON 内双引号需转义（`\"`）；PowerShell 可用单引号包裹整个 JSON。
仅支持 RGB 图层；智能对象上应用会记录为可重渲染的智能滤镜。建议把 `spektra/` 放在 CLI 二进制旁，或在任意目录运行前设置 env（第 3 级按 CWD 解析）。

## 版本对应

| fork 分支 | spektra-core | 资产包 |
|---|---|---|
| spektra-film-integration | <commit/tag> | v1 · SHA-256: <待补> |

资产与 spektra-core 版本必须严格对应；升级任一者须同步另一方，并用 golden 向量回归对拍。

## 数据来源

oracle 静态表与 profile JSON 的来源与许可（维护者填充，随附 `assets/spektra/LICENSES/`）：
- CMF / illuminant：CIE 公开数据（<待补版本/年份>）；spectra LUT / matrices / raw LUT / filters：上游生成脚本产出（<待补脚本/commit>）；profile JSON：上游测量/拟合数据生成（<待补>）。

## 许可证与归属

- `spektra-core` 及本 fork 构建整体适用 **GPL-3.0-or-later**（上游 spektrafilm 母本 GPL-3.0），与 PhotoCraft 的 MIT/Apache 双许可不兼容；分发时须按 GPL 提供源码与声明，并保留上游版权声明。
- 上游作者：Andrea Volpato 及 spektrafilm 贡献者（https://github.com/andreavolpato/spektrafilm）。
- 若需保持 MIT/Apache，应改为独立进程集成（路线 B/外部服务）。
