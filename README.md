# 随身新闻 · AstroBox 插件

为小米手环 10 和小米手环 10 Pro 的随身新闻快应用获取、分类并推送新闻。插件采用 Rust 与 AstroBox API Level 4，当前版本为 `1.0.0`。

开发者：一個小果冻、Nyx233。

## 功能

- 按来源和频道获取新闻，支持来源多选、跨来源去重及原文查看。
- 在手机本地完成新闻分类，选择 1–100 篇纯文字新闻推送到手环。
- 支持暂停、断点续传和设置记忆，手环收到后可离线阅读。

## 仓库结构

```text
PocketNews-AstroBox-Plugin/
|-- index.txt                 插件源入口，内容为 dist
|-- dist/
|   |-- manifest.json
|   |-- pocket_news.wasm
|   |-- icon.png
|   `-- licenses/
|-- src/                      Rust 源码与分类模型
|-- wit/                      AstroBox API Level 4 接口
|-- scripts/                  构建、源码打包与模型训练
|-- training/                 分类策略与训练说明
|-- tests/
|-- examples/
|-- licenses/
|-- Cargo.toml
|-- Cargo.lock
|-- manifest.json
|-- icon.png
|-- LICENSE
`-- THIRD_PARTY_NOTICES.md
```

## 构建与打包

需要 Python 3、Rust/Cargo 和 `wasm32-wasip2` 编译目标。在仓库根目录执行：

```powershell
rustup target add wasm32-wasip2
py -3 scripts/build.py
py -3 scripts/build.py --package
```

构建固定使用 Release 体积优化。`--package` 额外生成 `dist/PocketNews-<版本>.abp` 和独立的 `dist/PocketNews-<版本>-source.zip`，发布时同时提供安装包与源码 ZIP。源码包包含本仓库的源码、分类模型、构建脚本、测试与许可证，可独立重新构建。

脚本优先使用系统 Cargo，也支持本仓库 `.tools/` 或相邻 `news/.tools/` 中的本地 Rust 工具链。手环快应用单独位于相邻的 `news/` 工程，插件构建不需要快应用源码。

```powershell
cargo test --locked --release
py -3 scripts/package-source.py
```

分类模型与训练方法见 [training/README.md](training/README.md)，接口来源见 [wit/README.md](wit/README.md)。

## 插件源发布

目录按照 [AstroBox 官方插件仓库格式](https://abox.run/docs/creator-tools/resource-management/plugin-upload#step-2--创建并上传你的插件仓库)组织。根目录 `index.txt` 只有一行 `dist`，客户端需要的清单、WASM、图标和 `additional_files` 都在该目录中。

上传到自己的 GitHub 仓库时，保留 `dist/` 下的市场文件。`.gitignore` 仅排除构建缓存及可单独发布的 ABP、源码 ZIP，不忽略市场所需文件。随后按官方文档向插件源登记仓库的 raw 基地址。

项目采用 [AGPL v3](LICENSE)，第三方来源与许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。新闻内容版权归原发布者。
