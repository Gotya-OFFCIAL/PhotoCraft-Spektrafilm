# SpektraFilm 集成已知限制（2026-11-19 更新，SpektraLab 暗房工作台版）

## 1. ✅ 正片透射直扫与反相保护（2026-11-15 验证 / 2026-11-19 自动保护落地）
- **对拍裁决**：`parity_positive_velvia_scan_film` 用上游 Python oracle 导出的 Velvia 100 黄金向量对拍，**expose 7.8e-16 / develop 8.9e-16 / scan 1.4e-15 / 端到端 4.9e-15（机器精度）全过**。
- **原理与自动保护**：彩色反转正片（Velvia / Provia / Ektachrome / Kodachrome，`info.film_type == "positive"`）显影后本身已是正像，送入彩色负片相纸 `PrintPipeline` 会导致二次反相。无论 GUI 还是 CLI，`spektra_cmds::pipeline_for` 均自动为 `film_type == "positive"` 跳过负片相纸印相链、改走 `NegativeScanPipeline` 正片透射直扫；暗房工作台 UI（`spektra_ui.rs`）同步在选中正片时置灰相纸列表与放大机滑杆。
- **正片 DIR 成色剂边界**：`spektra-core::film::precompute` 当前仅移植了负片方向的成色剂分支（`silver = curves`），正片银盐反射分支（`nanmax - curves`）尚未移植。为保证输出正确，引擎侧（`spektra_cmds.rs`）与暗房 UI（`spektra_ui.rs`）对 `film_type == "positive"` 的胶片自动安全禁用 `couplers`。

## 2. 扩展暗房物理参数与空间颗粒状态
- **已开放的可选物理参数**：`couplers` / `couplersAmount`（负片 DIR 成色剂）、`inputGamut`（输入光谱色域压缩）、`outputGamut`（CAM16-UCS sRGB 输出色域压缩）、`printEv`（放大机印相曝光补偿）与 `ccCyan` / `ccMagenta` / `ccYellow`（放大机 C/M/Y 滤色头偏移）现已通过命令参数与新版三栏暗房工作台向用户开放。
- **向后兼容**：上述扩展参数默认均处于关闭或 `0` 状态，传入 `{}` 或传统 4 参数时的渲染结果与 v1 及黄金向量保持完全一致。
- **空间效应待后续版本**：物理胶片颗粒（空间随机模拟，golden_grain.json 已就位）与光晕（Halation）仍暂未接入交互管线，留待后续版本评估引入。

## 3. 管线缓存与零分配优化
- **oracle 静态表单例**：30MB 级 `Tables` 与 `OutputGamutCompressor` 查找表均通过 `OnceLock<Arc<...>>` 全局共享一次初始化，切换胶片/相纸或调节参数时不再重复加载表数据。
- **有界管线缓存**：`pipeline_for` 按完整光学/放大机参数指纹缓存最多 32 条管线实例，超出上限后自动清空重填，控制内存峰值。
- **零分配像素插值**：`PrintPipeline::process_image` 已切换至预计算缓冲区复用的 `process_pixel_at`，免去每像素 Vec 堆分配，大幅提升暗房滑杆实时预览帧率且保持逐比特精度一致。
- **中性 CC 自定位**：`neutral_print_filters.json` 优先从 `spektra-core` 资产根目录下的 `filters/` 自定位加载（兼容便携目录 `<exe dir>/spektra/oracle/filters/`），不再强依赖开发环境绝对路径。

## 4. 环境变量拼写
- 资产根覆盖变量名仍保持 `PHOTOCRAFT_SPEKTTRA_ASSETS`（SPEKTRA 拼成双 T 的 SPEKTTRA，与上游命名历史保持兼容）。

## 5. i18n
- 菜单文案 "Film Simulation…" 等界面译文随 `CRAFT_I18N_DIR` 构建嵌入；新版三栏暗房工作台当前以简体中文与英文专业暗房术语混排呈现（与 SpektraLab 术语一致）。
