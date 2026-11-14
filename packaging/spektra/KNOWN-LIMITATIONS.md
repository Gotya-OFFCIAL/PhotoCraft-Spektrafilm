# SpektraFilm 集成已知限制（2026-11-14，Route D v1 验收版）

## 1. 正片 stock 走负片管线（最重要）
- `precompute()` 的 `positive` 参数当前未使用（vendored 切片只覆盖负片预计算路径，
  4 个调用方全部传 `false`）。
- 28 个 profile 中含正片（slide film）：`fujifilm_velvia_100`、`fujifilm_provia_100f` 等
  cinema/正片 stock 会被当作负片处理，**输出不正确**。
- 上游 Python 母本在 `develop()` / `interp_density_cmy_layers()`（取负分支）等处分叉。
- **修复方向**：补 `film.rs` 正片分支 + 用 `tools/oracle` 生成正片 golden 向量对拍
  （见工作流分析 T11）。

## 2. 颗粒 / DIR 成色剂 / 色域压缩保持 v1 关闭语义
- v1 语义与 spektra-core M1/M2 对拍一致：这些空间/随机效应保持管线默认（关），
  保证输出可复现。开启需后续版本引入并重新对拍（golden_grain.json 已就位）。

## 3. 管线缓存组合上限
- `pipeline_for` 按 (film, paper, printChain) 缓存：28×6×2 = 336 组合理论上限。
- 每条管线持有一份 `Tables`（30MB oracle 静态表，`load_tables` 每组合重新加载）。
  极端情况（用户疯狂切换 stock 组合）内存可观；正常使用无感。可后续改为
  `Arc<Tables>` 全局共享（tables 与组合无关）。

## 4. 环境变量拼写
- 资产根覆盖变量名是 `PHOTOCRAFT_SPEKTTRA_ASSETS`（SPEKTRA 拼成双 T 的
  SPEKTTRA，与上游 Spektrafilm 命名史的笔误一致，代码与文档需保持一致）。

## 5. i18n
- 菜单文案 "Film Simulation…" 的 41 语言翻译行已产出（T08），待并入 craft-i18n
  仓库后随上游同步流程生效。
