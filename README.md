# 随身新闻同步器（PocketNewsSync）

为小米手环 10 和小米手环 10 Pro 的随身新闻快应用获取、分类并推送新闻。当前版本为 `1.0.0`，使用 AstroBox API Level 4，清单 WASI Version 为 `3`。

开发者：一個小果冻、Nyx233。

## 功能

- 按来源和频道获取新闻，支持来源多选、跨来源去重及原文查看。
- 在手机本地完成分类，选择 1–100 篇纯文字新闻推送到手环。
- 支持暂停、断点续传和设置记忆，手环收到后可离线阅读。

## 安装

下载 [PocketNewsSync-1.0.0.abp](https://github.com/Nyx-233/PocketNewsSync-AstroBox-Plugin/releases/download/v1.0.0/PocketNewsSync-1.0.0.abp)，在支持 API Level 4 的 AstroBox 中导入，并安装配套的随身新闻手环快应用。

## 仓库结构

本仓库的 `main` 分支用于发布插件，按照 [AstroBox 官方插件仓库格式](https://abox.run/docs/creator-tools/resource-management/plugin-upload)组织：

```text
PocketNewsSync-AstroBox-Plugin/
|-- index.txt
|-- README.md
|-- LICENSE
`-- dist/
    |-- manifest.json
    |-- pocket_news_sync.wasm
    `-- icon.png
```

`index.txt` 的内容为 `dist`。插件市场需要的清单、WASM 和图标均在该目录中，分类模型已嵌入 WASM。

## 重新打包

在本仓库根目录运行以下命令，即可用现有 WASM 生成 ABP，仅需 Python 3：

```powershell
py -3 scripts/build.py --package
```

## 完整源码

[下载 1.0.0 完整源码 ZIP](https://github.com/Nyx-233/PocketNewsSync-AstroBox-Plugin/releases/download/v1.0.0/PocketNewsSync-1.0.0-source.zip)。源码包包含 Rust 源码、分类模型、WIT、训练策略、构建脚本、测试及项目 LICENSE，可独立重新构建。

需要 Python 3、Rust/Cargo 和 `wasm32-wasip2` 编译目标。解压源码包后，在源码根目录执行：

```powershell
rustup target add wasm32-wasip2
py -3 scripts/build.py --package
```

安装包与完整源码均可在 [Releases](https://github.com/Nyx-233/PocketNewsSync-AstroBox-Plugin/releases) 获取。

项目采用 [AGPL v3](LICENSE)。新闻内容版权归原发布者。
