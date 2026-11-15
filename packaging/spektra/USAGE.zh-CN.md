# SpektraFilm 光谱胶片模拟（中文使用说明）

SpektraFilm 不是 LUT 滤镜，而是按 81 波长物理建模的光谱模拟：负片 → 冲洗 →（可选放大机）→ 相纸 → 扫描。

## 菜单路径

**滤镜 → SpektraFilm → Film Simulation…**

## 参数说明

对话框里 `film` / `paper` 是**下拉选择框**（列出全部可用 stock，默认已选好），`ev` 是滑杆，`printChain` 是开关。勾选「预览」实时看效果。

| 参数 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `film` | 下拉 | `kodak_portra_400` | 胶片 stock，22 选 1 |
| `paper` | 下拉 | `kodak_portra_endura` | 相纸 stock，**仅当 `printChain = true`（默认）时生效** |
| `ev` | 滑杆 | `0` | 曝光补偿，范围 `-4` ~ `+4` |
| `printChain` | 开关 | **`true`** | `true` = 相纸输出（**胶片观感**，默认；负片 → 放大机 → 相纸 → 扫描，即上游 `scan_film=false` 语义）；`false` = 负片直扫（**橙罩原始扫描**，研究用途，不是可用的滤镜观感） |

提示：滤镜是**叠加在当前像素状态上的**——在已应用过胶片模拟的图层上再开一次会二次成像（颜色会极端化）。想换 stock 重来，先 `Ctrl+Z` 撤销上一次，或在「历史记录」面板回到应用前的状态。

提示：「相纸输出（胶片观感）」开关**每次打开对话框都从默认勾选开始**，不会被上次的参数记住（`printChain` 不参与参数记忆）。如果画布看起来是橙色的负片扫描，先确认这个开关是勾选状态。

## 可用 stock

对话框下拉列表与 `assets/spektra/oracle/profiles/` 的 **28 个 JSON** 一一对应：22 个胶片 stock + 6 个相纸 stock。

**胶片栏（`film`，22）**：`kodak_portra_160/400/800`（含 `800_push1/800_push2` 追冲）、`kodak_ektar_100`、`kodak_gold_200`、`kodak_ultramax_400`、`kodak_kodachrome_64`、`kodak_ektachrome_100`（正片）、`fujifilm_pro_400h`、`fujifilm_c200`、`fujifilm_xtra_400`、`fujifilm_velvia_100`、`fujifilm_provia_100f`（正片）、`kodak_vision3_50d/200t/250d/500t`（电影片）、`kodak_verita_200d`、`kodak_2383/2393`（电影拷贝）。

**相纸栏（`paper`，6）**：`kodak_portra_endura`、`kodak_endura_premier`、`kodak_supra_endura`、`kodak_ultra_endura`、`kodak_ektacolor_edge`、`fujifilm_crystal_archive_typeii`。

## CLI 批处理示例

单图扫描链（负片直扫，Portra 400）：

```bash
photocraft-cli run input.png   --cmd filter.spektra.filmSim   --params '{"film":"kodak_portra_400","ev":0}'   --out output_portra.png
```

单图 print 链（完整暗房链，Portra 400 + Endura 相纸）：

```bash
photocraft-cli run input.png   --cmd filter.spektra.filmSim   --params '{"film":"kodak_portra_400","paper":"kodak_portra_endura","printChain":true}'   --out output_portra_print.png
```

整目录批处理（batch 走 actions JSON 步骤文件）：

```bash
# spektra.json
# {"actions":[{"name":"spektra-film","steps":[
#   {"command":"filter.spektra.filmSim","params":{"film":"kodak_portra_400"}}
# ]}]}
photocraft-cli batch --actions spektra.json --in ./images --out ./results
```


## 注意事项

- **智能滤镜**：支持作为智能滤镜应用，参数可随时重新编辑。
- **选区**：支持在选区内应用，仅影响选中区域。
- **图层锁定**：对锁定的图层应用时会报错，请先解锁目标图层。
- **资产目录**：可通过环境变量 `PHOTOCRAFT_SPEKTTRA_ASSETS` 指定资产目录（指向包含 `profiles/` 的目录），未设置时使用默认安装路径。