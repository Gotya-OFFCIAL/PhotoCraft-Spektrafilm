# SpektraFilm 集成已知限制（2026-11-15 更新，Route D v1 验收版）

## 1. ✅ 正片直扫已验证正确（2026-11-15 更正昨夜的误判）
- **昨夜误判**：曾认为正片 stock（Velvia/Provia 等）「走负片管线输出不正确」。
- **对拍裁决**：`parity_positive_velvia_scan_film`（2026-11-15 新增）用上游 Python
  oracle 导出的 Velvia 100 黄金向量对拍，**expose 7.8e-16 / develop 8.9e-16 /
  scan 1.4e-15 / 端到端 4.9e-15（机器精度）全过**。
- **原理**：上游 Python 母本的 scan 链（expose → develop → scan）对 positive
  **没有独立分支**；正片语义完全由 profile 数据（曲线形状/基底密度/窗口参数）承载。
  三个真正的 positive 分支点是 DIR 成色剂（silver = nanmax − 曲线 反射）、
  颗粒层插值（取负）、扫描仪黑白校正参考——**v1 全部处于关闭状态**，不影响本滤镜。
- **残留事项**：未来为正片 stock 启用成色剂/颗粒时，需先移植对应 positive 分支
  （`precompute()` 的 `positive` 参数签名已预留；上游算法见
  `couplers.py::compute_density_curves_before_dir_couplers`、
  `density_curves.py::interp_density_cmy_layers`）。
- 黄金向量：`assets/spektra/oracle/golden_vectors_velvia.json`（由
  `tools/oracle/golden_vectors.py` 以 `ORACLE_FILM_STOCK=fujifilm_velvia_100` 导出）。

## 2. 颗粒 / DIR 成色剂 / 色域压缩保持 v1 关闭语义
- v1 语义与 spektra-core M1/M2 对拍一致：这些空间/随机效应保持管线默认（关），
  保证输出可复现。开启需后续版本引入并重新对拍（golden_grain.json 已就位）。

## 3. 管线缓存
- `pipeline_for` 按 (film, paper, printChain) 缓存：28×6×2 = 336 组合理论上限。
- 2026-11-15 已优化：oracle `Tables`（30MB）经 `Arc` 全局单例共享，组合切换不再
  重复加载；缓存内只存管线自身小结构，内存上界 = 30MB + 组合数×管线小结构。

## 4. 环境变量拼写
- 资产根覆盖变量名是 `PHOTOCRAFT_SPEKTTRA_ASSETS`（SPEKTRA 拼成双 T 的
  SPEKTTRA，与上游 Spektrafilm 命名史的笔误一致，代码与文档需保持一致）。

## 5. i18n
- 菜单文案 "Film Simulation…" 的 41 语言翻译行已产出（T08），待并入 craft-i18n
  仓库后随上游同步流程生效。
