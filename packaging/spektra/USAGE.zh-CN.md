# SpektraFilm 光谱胶片模拟（中文使用说明）

SpektraFilm 不是 LUT 滤镜，而是按 81 波长物理建模的光谱模拟：负片 → 冲洗 →（可选放大机）→ 相纸 → 扫描。

## 菜单路径

**滤镜 → SpektraFilm → Film Simulation…**

## 参数说明

| 参数 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `film` | string | `kodak_portra_400` | 胶片 stock，见下方「可用 stock」 |
| `paper` | string | `kodak_portra_endura` | 相纸 stock，**仅当 `printChain = true` 时生效**；负片直扫模式下忽略 |
| `ev` | float | `0` | 曝光补偿，范围 `-4` ~ `+4` |
| `printChain` | bool | `false` | `false` = 负片直扫；`true` = 完整暗房链（负片 → 放大机 → 相纸 → 扫描） |

## 可用 stock

可用列表以 `assets/spektra/oracle/profiles/` 目录下实际 **28 个 JSON 文件**为准。

**胶片栏（`film`）**：`kodak_portra_400`、`kodak_portra_800`、`fujifilm_pro_400h`、`fujifilm_velvia_100`、`fujifilm_provia_100f`、`fujifilm_c200` 等。

**相纸栏（`paper`）**：`kodak_portra_endura`、`fujifilm_crystal_archive_typeii` 等。

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