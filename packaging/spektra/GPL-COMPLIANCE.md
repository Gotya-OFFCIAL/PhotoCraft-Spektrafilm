# SpektraFilm 集成 GPL 合规清单

（由七牛云 conatus-free 子代理起草，2026-11-14）

# spektra-core GPL-3.0-or-later 合规清单（PhotoCraft fork）

## 1. 构建产物许可证状态（静态链接 = 衍生作品）
- [ ] `crates/engine` 进程内链接 `spektra-core` → 整个二进制为 GPL-3.0-or-later 衍生作品
- [ ] 分发产物整体以 GPL-3.0-or-later 提供，不得仅以 MIT/Apache 发布
- [ ] 分发时提供完整对应源码（含 `crates/spektra-core/` 与 `crates/engine/src/spektra_cmds.rs`）
- [ ] 当前个人使用不触发 GPL 义务（GPL 仅约束 conveyance）

## 2. NOTICE / ATTRIBUTION（粘贴到 `NOTICE`）
```
PhotoCraft 包含以下 GPL-3.0-or-later 组件：

spektra-core（vendored，crates/spektra-core/）
Copyright (C) turbasvin (spektrafilm-rust)
母本 Copyright (C) Andrea Volpato (spektrafilm, Python)
License: GPL-3.0-or-later https://www.gnu.org/licenses/gpl-3.0.html
Source: https://github.com/turbasvin/spektrafilm-rust

PhotoCraft contains GPL-3.0-or-later components:

spektra-core (vendored, crates/spektra-core/)
Copyright (C) turbasvin (spektrafilm-rust)
Parent Copyright (C) Andrea Volpato (spektrafilm, Python)
License: GPL-3.0-or-later https://www.gnu.org/licenses/gpl-3.0.html
Source: https://github.com/turbasvin/spektrafilm-rust
```

## 3. `crates/spektra-core/Cargo.toml`
```toml
[package]
name = "spektra-core"
license = "GPL-3.0-or-later"
# 不写 lints.workspace = true

[lints.rust]
unsafe_code = "forbid"
```
- [ ] `license = "GPL-3.0-or-later"`（SPDX，勿写 MIT/Apache）
- [ ] 不写 `lints.workspace = true`，改用本 crate 内 `[lints]` 表

## 4. 公开发布（GitHub public repo）必须 / 禁止
必须：
- [ ] 整体以 GPL-3.0-or-later 发布源码与构建产物
- [ ] 保留 `crates/spektra-core/` 上游版权声明与 LICENSE
- [ ] 提供构建说明（`cargo build --release`）与 vendored 源码
- [ ] README 声明：构建产物为 GPL-3.0-or-later
- [ ] 对 `spektra_cmds.rs` 等修改加显著变更声明
禁止：
- [ ] 不得仅以 MIT/Apache 发布二进制或 crate
- [ ] 不得移除/篡改上游版权声明
- [ ] 不得对 GPL 代码添加额外限制（GPL §10）
- [ ] 不得闭源分发 spektra-core 及链接它的代码

## 5. CC BY-SA 4.0 数据署名（胶片 profile JSON）
- [ ] 在 `assets/film-profiles/NOTICE` 注明：数据来源、作者、CC BY-SA 4.0、链接
- [ ] 保留原始署名与许可证标识，注明对数据的修改
- [ ] 改编后的 profile 数据以 CC BY-SA 4.0 共享（SA 条款）