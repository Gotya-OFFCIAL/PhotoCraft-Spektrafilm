# SpektraFilm 资产部署（PhotoCraft fork · spektra-film-integration 分支）

本分支通过源码级集成（路线 D）把 [spektrafilm](https://github.com/andreavolpato/spektrafilm)
的物理胶片模拟（spektrafilm-rust 的 `spektra-core`，GPL-3.0-or-later）接入 PhotoCraft，
提供 `filter.spektra.filmSim` 滤镜（菜单：**Filter › SpektraFilm › Film Simulation…**）。

运行时需要配套的光谱数据资产（源码树内为 `assets/spektra/`，约 30 MB）：

```
spektra/
├── oracle/          # oracle 静态表：cmfs/illuminants/spectra LUT/matrices/raw LUT + 28 个 profile JSON
│   ├── profiles/
│   └── *.bin / *.json
└── (与 photocraft.exe 同级部署)
```

注：仓库内 `assets/spektra/oracle/` 还含少量 parity 测试夹具（`golden_*.json`、`debug_print_intermediates_*.bin`，合计 <0.5 MB），
运行时不需要，随包部署无害。

## 部署

把 `assets/spektra/` 整个目录复制到 **可执行文件同级**：

```
<PhotoCraft 目录>/
├── photocraft.exe
├── spektra/          ← 复制自仓库 assets/spektra/
│   └── oracle/...
```

滤镜首次运行时按 `<exe 目录>/spektra/` 解析资产；找不到时可用环境变量覆盖根目录：

```
set PHOTOCRAFT_SPEKTTRA_ASSETS=D:\some\path\spektra
```

（根目录需同时包含 `oracle/` 子目录。）

## 许可证

`spektra-core` 为 GPL-3.0-or-later（spektrafilm 母本 GPL-3.0）。vendoring 进本 fork 后，
**本 fork 的构建整体适用 GPL-3.0-or-later**，与上游 PhotoCraft 的 MIT/Apache 双许可不兼容；
分发时需按 GPL 提供源码与声明。若需保持 MIT/Apache，应改为独立进程集成（路线 B/外部服务）。

## 参数

| 参数 | 说明 | 默认 |
|---|---|---|
| `film` | 胶片 stock 名（`spektra/oracle/profiles/` 下的 JSON 文件名，不含扩展名），如 `kodak_portra_400`、`fujifilm_velvia_100` | `kodak_portra_400` |
| `paper` | 相纸 stock 名，仅 `printChain` 开启时使用 | `kodak_portra_endura` |
| `ev` | 曝光补偿 -4..4（线性输入按 2^ev 缩放） | `0` |
| `printChain` | 完整 print 链（负片→相纸→扫描）；关闭则仅负片直扫 | `false` |

v1 语义与 spektra-core 黄金向量对拍一致：颗粒/DIR 成色剂/色域压缩保持管线默认（关）。
仅支持 RGB 图层；智能对象上应用会记录为可重渲染的智能滤镜。
