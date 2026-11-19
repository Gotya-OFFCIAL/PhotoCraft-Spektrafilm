# SpektraFilm 光谱胶片模拟（中文使用说明）

SpektraFilm 不是普通 LUT 滤镜，而是按 **81 波长（380–780 nm）** 物理建模的光谱胶片暗房管线：相机曝光（可选输入色域压缩） → 负片/正片显影（可选 DIR 成色剂） → 放大机（TH-KG3 光源、Print EV 与 C/M/Y 滤色头） → 相纸印相 / 直扫 → CAM16-UCS 色域压缩与扫描输出。

## 入口路径

- **标准菜单**：`滤镜 → SpektraFilm → Film Simulation…`
- **`PhotoCraft-Spektrafilm` 专属快捷入口**：
  - 顶部选项栏：`✦ SpektraFilm 胶片暗房` 琥珀金按钮（打开文档后可用）
  - 右侧属性/调整面板：`✦ SpektraFilm 光谱胶片暗房` 入口按钮

## SpektraLab 风格三栏暗房工作台

打开「Film Simulation…」后即进入 1120×700 三栏暗房工作台：

1. **顶栏（Top Bar）**：
   - **预设配方**：一键切换 `Portra 人像`、`Velvia 风光`、`富士 400H 日系`、`Vision3 电影`、`Gold 暖阳` 5 组典型胶片/相纸组合；
   - **视图对比**：`显影效果 Process` / `分屏对比 Split` / `原图 Original`；
   - **视口缩放**：`−` / `Fit`（或当前百分比）/ `+`（亦支持鼠标滚轮缩放与画布拖拽平移）。
2. **左栏（胶片与印相 · Film & Print）**：
   - **分类筛选 & 搜索**：按 `全部 / 负片 / 正片 / 电影` 过滤或按名称检索，支持开关 **自动配纸**（按胶片类型自动配对推荐相纸）；
   - **胶片卷库（22 种）**：展示 11 张嵌入式胶片盒封面图与双色片基徽标、电影片 `CINE` 徽章；
   - **印相相纸（6 种）与直扫**：支持 6 款彩色相纸及「负片直扫（Direct Scan）」研究模式。
3. **中栏（暗房取景器）**：
   - **分屏拖动**：`Split` 模式下可直接左右拖动垂直分界线，实时对照原图与冲印结果；
   - **按住对比**：`Process` 模式下鼠标按住画布可临时查看原图；
   - **右上角示波器**：实时显示 64-bin R/G/B + 亮度（Luma）直方图，底部显示 81 波长光谱链路状态条。
4. **右栏（暗房物理参数）**：
   - 支持 `全部` / `显影前 Pre-Dev` / `印相 Post-Dev` 分组切换与 **重置参数 ↺**；
   - 包含 **曝光宽容度曲线与超限比例统计**、**相机曝光补偿 EV**（带 `As Shot` 与 `±1/3 EV` 步进按钮）、**输入光谱色域压缩**、**DIR 成色剂与强度**（正片自动安全禁用）、**TH-KG3 放大机中性 CC 读数 + Print EV + C/M/Y 滤色头**、**CAM16-UCS 输出色域压缩**，以及实时预览开关。

## 参数说明（GUI / CLI 通用）

| 参数 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `film` | 枚举/字符串 | `kodak_portra_400` | 胶片 stock，22 选 1 |
| `paper` | 枚举/字符串 | `kodak_portra_endura` | 相纸 stock，6 选 1（仅负片/电影片且 `printChain = true` 时生效） |
| `ev` | 滑杆/浮点 | `0` | 相机曝光补偿，范围 `-4` ~ `+4` |
| `printChain` | 布尔开关 | **`true`** | `true` = 负片/电影片暗房相纸输出（默认，胶片观感）；`false` = 负片直扫（橙罩原片，研究用途）。**注**：正片/反转片（Velvia / Provia / Ektachrome / Kodachrome）本身为正像，无论该参数或预设如何均自动跳过负片相纸反相链，直接走正片透射直扫 |
| `couplers` | 布尔开关 | `false` | 启用负片 DIR 成色剂（层间化学抑制效应；正片 stock 自动安全禁用） |
| `couplersAmount` | 滑杆/浮点 | `1` | DIR 成色剂强度，范围 `0` ~ `2`（仅 `couplers = true` 时生效） |
| `inputGamut` | 布尔开关 | `false` | 输入光谱色域压缩（缓解极端高饱和色彩溢出） |
| `outputGamut` | 布尔开关 | `false` | 输出 sRGB CAM16-UCS 色域压缩 |
| `printEv` | 滑杆/浮点 | `0` | 放大机相纸曝光补偿，范围 `-2` ~ `+2`（仅实际走相纸输出时有效） |
| `ccCyan` | 滑杆/浮点 | `0` | 放大机青色（Cyan）滤色片偏移，范围 `-50` ~ `+50` |
| `ccMagenta` | 滑杆/浮点 | `0` | 放大机品红（Magenta）滤色片偏移，范围 `-50` ~ `+50` |
| `ccYellow` | 滑杆/浮点 | `0` | 放大机黄色（Yellow）滤色片偏移，范围 `-50` ~ `+50` |

> **提示**：滤镜会**叠加在当前像素状态上**——在已应用过胶片模拟的图层上再开一次会二次成像（颜色会极端化）。想换 stock 重来，请先 `Ctrl+Z` 撤销或使用智能滤镜重新编辑。
>
> **提示**：`printChain`（相纸输出）不参与参数记忆——每次打开对话框均从默认的相纸输出（`true`）开始。选中彩色反转正片时，左侧相纸与右侧放大机滑杆会自动置灰保护，改走正片透射直扫，避免二次反相；选回负片/电影片即自动恢复相纸输出。

## 可用 stock（28 个 Measured Profile）

对话框列表与 `spektra/oracle/profiles/` 中的 **28 个 JSON** 一一对应：

- **正片 / 反转片（Positive，4 种）**：`fujifilm_provia_100f`、`fujifilm_velvia_100`、`kodak_ektachrome_100`、`kodak_kodachrome_64`
- **彩色负片（Negative，11 种）**：`fujifilm_c200`、`fujifilm_pro_400h`、`fujifilm_xtra_400`、`kodak_ektar_100`、`kodak_gold_200`、`kodak_portra_160`、`kodak_portra_400`、`kodak_portra_800`、`kodak_portra_800_push1`、`kodak_portra_800_push2`、`kodak_ultramax_400`
- **电影负片 / 印片（Cine，7 种）**：`kodak_verita_200d`、`kodak_vision3_50d`、`kodak_vision3_200t`、`kodak_vision3_250d`、`kodak_vision3_500t`、`kodak_2383`、`kodak_2393`
- **暗房相纸（Paper，6 种）**：`kodak_portra_endura`、`kodak_supra_endura`、`kodak_ultra_endura`、`kodak_endura_premier`、`kodak_ektacolor_edge`、`fujifilm_crystal_archive_typeii`

## CLI 批处理示例

默认参数（Portra 400 + Portra Endura 相纸输出）：

```bash
photocraft-cli run input.png --cmd filter.spektra.filmSim --params '{}' --out output_portra.png
```

完整暗房物理参数示例（Vision3 500T + Supra Endura + DIR 成色剂 + 放大机暖调偏黄配光 + 双色域压缩）：

```bash
photocraft-cli run input.png --cmd filter.spektra.filmSim --params '{"film":"kodak_vision3_500t","paper":"kodak_supra_endura","couplers":true,"couplersAmount":1.0,"ccYellow":8.0,"printEv":0.2,"inputGamut":true,"outputGamut":true}' --out output_cinema.png
```

负片直扫（仅研究/导出负片橙罩原片）：

```bash
photocraft-cli run input.png --cmd filter.spektra.filmSim --params '{"film":"kodak_portra_400","printChain":false}' --out output_neg_scan.png
```

整目录批处理（`batch` 走 actions JSON 步骤文件）：

```bash
# spektra.json
# {"actions":[{"name":"spektra-film","steps":[
#   {"command":"filter.spektra.filmSim","params":{"film":"kodak_portra_400","couplers":true}}
# ]}]}
photocraft-cli batch --actions spektra.json --in ./images --out ./results
```

## 注意事项

- **免环境变量便携运行**：资产目录已部署在可执行文件旁 `spektra/`，无需配置环境变量即可运行。如需指向自定义资产目录，可设置 `PHOTOCRAFT_SPEKTTRA_ASSETS`（注意双 T 拼写，指向含 `oracle/` 或 `profiles/` 的目录）。
- **便携配置**：独立版目录含 `portable.txt`，用户配置（含默认暗房 Studio 深色主题与简体中文界面）保存在本地 `PhotoCraftData/preferences.json`，不会写入系统 `%APPDATA%`。
- **智能滤镜 / 选区 / 锁定图层**：完整支持作为智能滤镜非破坏性应用、按当前选区羽化权重混合，以及像素锁定保护。
