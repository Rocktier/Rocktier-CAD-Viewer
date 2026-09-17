# Microsoft Store Listing — Rocktier CAD Viewer

> 照 `Rocktier-Windows-Store-上架避坑指南.md` 逐条规避；本文末尾有对照自查表。

---

## 0. 提交前必须对齐的两件事（首次提交最容易卡这里）

### 0.1 包标识已与 Partner Center 对齐 ✅

Partner Center「产品标识」实测值（2026-09-16 核对），已逐项写入代码：

| Partner Center 字段 | 值 | 写入位置 | 状态 |
|---|---|---|---|
| `Package/Identity/Name` | `Rocktier.RocktierCADViewer` | `src-tauri/tauri.conf.json` → `identifier` | ✅ 已对齐 |
| `Package/Identity/Publisher` | `CN=4EA39D7A-401B-4D56-98D0-8ECB1F2B8DF7` | `src-tauri/gen/windows/bundle.config.json` → `publisher` | ✅ 本来就一致 |
| `Package/Properties/PublisherDisplayName` | `Rocktier` | 同上 → `publisherDisplayName` | ✅ 本来就一致 |
| Package Family Name (PFN) | `Rocktier.RocktierCADViewer_e0sb54jawjj5c` | 只读，引用应用时使用 | — |
| Store ID | `9NBZS9SGWQ9R` | 只读，商店链接使用 | — |

> ⚠️ **v0.1.0 的 MSIX 不能用于提交**：它的 `Identity/Name` 还是旧的 `studio.rocktier.cadviewer`，
> 上传会直接报标识不匹配。已把标识改对、版本升到 **0.1.1** 并重新构建。
>
> 家族先例：PDF Squeeze 用的是 `Rocktier.RocktierPDFSqueeze`（微软分配风格）—— 本项目现在与之同构。

### 0.2 磁贴必须是当前设计

包内/安装器里的磁贴图标如果还是旧素材，会直接命中政策 **10.1.1.11（磁贴模糊或非自定义）**。

本项目此前 `Square*Logo.png` / `StoreLogo.png` / `64x64.png` / `icon.png` 停留在 2026-09-11 的旧版本，**早于当前 `icon.svg`**；已于 2026-09-16 全部由当前矢量源重新生成，`icon.ico` 也重做（见第 8 节）。

### 0.3 两个已核验、无需改动的点

- **清单语言资源**：`AppxManifest.xml` 目前只声明 `<Resource Language="en-us" />`。应用内的中英切换是运行时的，与清单无关；**不建议**在没有对应资源包的情况下自行添加 `zh-cn`，声明了却没有资源反而会被校验挑出来。
- **权限声明**：清单里只有打包器自动附加的 `runFullTrust`，**没有 `internetClient`** —— 这与"纯离线、不上传"的宣称一致，是加分项。

---

## 1. 应用信息

| 字段 | 内容 |
|------|------|
| **应用名称** | Rocktier CAD Viewer |
| **发布者显示名称** | Rocktier |
| **包标识（Identity Name）** | `Rocktier.RocktierCADViewer`（Partner Center 分配，已写入 `tauri.conf.json` 的 `identifier`） |
| **Publisher** | `CN=4EA39D7A-401B-4D56-98D0-8ECB1F2B8DF7`（与 PDF Squeeze 同一 Partner Center 账号，已写入 `src-tauri/gen/windows/bundle.config.json`） |
| **定价** | US$4.99 买断（一次性） |
| **试用** | **不启用**（纯买断）。代码里没有任何 license/trial 逻辑，启用试用等于白送；描述文案也据此写为一次性买断 |
| **隐私政策 URL** | `https://rocktier.com/privacy` |
| **支持联系邮箱** | hello@rocktier.com |
| **支持 URL** | `https://rocktier.com/` |

---

## 2. English listing

> ⚠️ **本节的文案已于 2026-09-17 更新**（新增「Small and self-contained」体积说明；把「Try it for 7 days」改为一次性买断）。
> 但**商店里的列表仍是当天提交审核的旧版**——**这些改动随下一个版本一起提交，不要为此单独重发**。
> 提交下一版时，只需把本节的两段文案替换进 Partner Center 的对应字段即可。

### Short description

> （上限 100 字符，本行 86 字符）

```
Open DWG and DXF drawings in seconds. Fast, offline CAD viewer — no CAD suite required.
```

### Long description

> Rocktier CAD Viewer opens the drawings your colleagues send you — DWG and DXF — in seconds, on your own PC.
>
> No CAD suite to install, no license server to reach, no cloud viewer to upload to. You double-click a drawing and it is on screen.
>
> **How it works**
>
> 1. Open a .dwg or .dxf file (drag it in, or use the file picker)
> 2. Pan and zoom through the whole drawing — it redraws immediately, however far in you go
> 3. Measure distances, read coordinates, and toggle layers from the side panel
>
> **What you get**
>
> - **Opens DWG and DXF** — files from current releases down to older formats
> - **Fast at any zoom** — GPU-accelerated rendering keeps large drawings responsive
> - **Measure and inspect** — snap to endpoints and midpoints, check distances and coordinates
> - **Layer control** — list, search, show or hide layers, and read each layer's colour
> - **Paper space layouts** — view each layout the way it was plotted
> - **Private by design** — drawings are processed entirely on your device. Nothing is uploaded, and there is no telemetry of any kind
> - **Works offline** — no account, no sign-in, no connection needed
> - **Small and self-contained** — about 10 MB to install, and that already includes its own DWG engine. No bundled runtime, no background service, nothing left running once you close the window
>
> **Built for**
>
> Checking a drawing a client just sent. Confirming a dimension before you quote. Showing a plan in a meeting without booting a full CAD workstation.
>
> This is a viewer, on purpose: it reads drawings, measures them and inspects them, and it never writes to your originals.
>
> English and Chinese (Simplified) interface.
>
> One-time purchase — no subscription, no renewal. The app does not phone home, so
> there is nothing to sign in to and nothing that expires.

### Release notes (v0.1.0)

> First release.
>
> - Opens DWG and DXF drawings
> - GPU-accelerated pan and zoom
> - Distance measuring with endpoint/midpoint snapping
> - Layer panel with colour and visibility control
> - Paper space layouts
> - Fully offline, no telemetry

---

## 3. 分类

| 字段 | 选择 |
|------|------|
| **类别** | 多媒体设计（Multimedia design）— 备选：生产力（Productivity） |
| **子类别** | 设计与插画 / 工程（若该类别下无可选项则留空） |

> CAD 看图工具归在"多媒体设计"更贴近用户预期；若审核对该类别有额外要求，退回"生产力"即可。

---

## 4. 搜索关键词（Keywords）

> **规则（踩坑 #16）：不得出现任何平台名或其他产品名**（MacOS / Windows / Linux / AutoCAD 等）。

```
DWG viewer, DXF viewer, CAD viewer, drawing viewer, DWG reader, CAD file viewer,
blueprint viewer, technical drawing viewer, CAD measure, layer viewer,
CAD看图, 图纸查看, DWG查看器, DXF查看器
```

---

## 5. 截图

要求：桌面屏幕截图，**1366×768 以上**，至少 1 张（建议 4–6 张）。本仓库 `store-assets/` 内：

| 文件 | 尺寸 | 画面 |
|------|------|------|
| `screenshot-1-drawing.png` | 2560×1544 | 打开图纸 — 完整平面图 + 图层面板 |
| `screenshot-2-empty.png` | 2560×1544 | 空状态 — 品牌文案与打开入口 |
| `hero-1920x1080.png` | 1920×1080 | 16:9 商店推广图（可选） |

> 截图为「仅捕获应用窗口」后裁掉 macOS 标题栏的结果，画面内不含任何第三方或个人内容。
> 图纸使用 `store-assets/office-plan.dxf`（本仓库脚本 `make-demo-plan.py` 生成的**合成图纸**）。
> **不要**用 `testdata/real/` 里的真实客户图纸做公开截图 —— 客户图纸名一旦出现在「最近打开」列表里，也会随空状态截图一起泄露。

---

## 6. 商店徽标（Store listing → 徽标）

| 图片类型 | 尺寸 | 文件 | 备注 |
|---------|------|------|------|
| 1:1 应用磁贴图标 | 300×300 | `store-assets/store-tile-300.png` | **必传**，用于解决踩坑 #17 |
| 16:9 超级英雄图片 | 1920×1080 | （可选，未生成） | 仅用于商店推广位 |

> 包内磁贴（`src-tauri/gen/windows/Assets/`）由 MSIX 使用，**不需要**在 Partner Center 单独上传。

---

## 7. 附加信息

| 字段 | 内容 |
|------|------|
| **最低系统要求** | Windows 10 版本 1809（build 17763）或更高 / Windows 11 |
| **架构** | x64 |
| **安装包类型** | MSIX（商店分发）；MSI / NSIS 供官网分发 |
| **是否包含广告** | 否 |
| **是否包含应用内购买** | 否 |
| **需要网络连接** | 否（纯离线应用） |
| **声明权限** | 无网络权限（仅 `runFullTrust`，由打包工具自动附加） |
| **界面语言** | 英语、简体中文 |

---

## 8. 上架前准备（本仓库已完成的自动化部分）

| 项 | 状态 |
|---|---|
| `icon.ico` 含 16/24/32/48/64/128/256 七种尺寸 | ✅ 由矢量逐尺寸渲染（非缩放），已替换 `src-tauri/icons/icon.ico` |
| **平台图标/磁贴与当前 `icon.svg` 同源** | ✅ 2026-09-16 重新生成全部 `Square*Logo.png`、`StoreLogo.png`、`64x64.png`、`icon.png`（此前停留在 09-11 旧版，命中 10.1.1.11 风险） |
| MSIX 包内磁贴（Square150/44、StoreLogo、Wide310x150） | ✅ `src-tauri/gen/windows/Assets/` |
| Partner Center 300×300 磁贴 | ✅ `store-assets/store-tile-300.png` |
| MSIX 打包配置（Publisher / 文件关联 / 权限） | ✅ `src-tauri/gen/windows/bundle.config.json` |
| GitHub Actions 自动构建 MSI + MSIX | ✅ `.github/workflows/build.yml` |
| 随包 LibreDWG（`dwg2dxf.exe`）在 CI 中自动获取 | ✅ `scripts/provision-libredwg-windows.ps1 -Download` |
| 隐私政策链接可达 | ✅ `https://rocktier.com/privacy` 实测 HTTP 200 |

### 仍需在 Windows / Partner Center 上人工完成

- [ ] 在 Partner Center 创建产品，拿到 **Identity Name / Publisher / Publisher Display Name**，与上面 `bundle.config.json` 对齐
- [ ] 本地跑一次 **WACK**（Windows App Certification Kit）
- [ ] 上传 300×300 磁贴 + 4 张截图
- [ ] 填写描述、关键词、分类、定价与试用
- [ ] 用真实 Windows 机器复核一次"安装 → 打开图纸"（MSIX 安装后 `resources/libredwg/dwg2dxf.exe` 是否被正确解出）

---

## 9. 年龄分级（IARC 问卷）怎么填

Partner Center → 年龄分级 → 开始问卷调查。本项目是**纯离线单机看图工具**：无账号、无联网、
无广告、无应用内购买、不收集数据，因此**所有内容类问题一律选「否 / 无」**，会拿到各区域最低分级。

| 问卷问题（大意） | 选择 | 依据 |
|---|---|---|
| 内容类别 | 非游戏（实用工具 / 生产力） | 看图工具，不是游戏 |
| 暴力 / 血腥 | 否 | 无任何暴力内容 |
| 性 / 裸露 | 否 | — |
| 粗俗语言 | 否 | — |
| 烟酒毒品等受管物质 | 否 | — |
| 赌博（含模拟赌博） | 否 | — |
| 恐怖 / 惊吓 | 否 | — |
| 用户生成内容 / 社交 | 否 | 只打开本地图纸；无账号、无分享、无聊天 |
| 用户间交互 / 多人 | 否 | 无联网功能 |
| 位置共享 | 否 | 不读取位置 |
| 收集个人信息 | 否 | 不收集、不上传（与清单里无 `internetClient` 一致） |
| 应用内数字商品购买 | 否 | **应用内无购买流程**；试用与买断由商店渠道完成，不是 IAP |
| 广告 | 否 | 无广告、无第三方 SDK |
| 无限制网络访问 / 内置浏览器 | 否 | 纯离线；官网与反馈链接交给系统浏览器打开 |

**预期结果**：ESRB Everyone · PEGI 3 · USK 0 · CERO A · GRAC 全体 · ClassInd Livre · ACB G
（各区域最低分级，可一遍通过）。

> ⚠️ 问卷答案是有约束力的申报，必须与应用实际行为一致；日后若加入联网、账号或购买功能，必须回来重填。
> 提交后 Partner Center 会按区域自动套用分级，无需逐地区单独填写。

---

## 10. 踩坑对照自查

| 踩坑 | 内容 | 本项目处理 |
|------|------|-----------|
| #1 | cargo 镜像用 TUNA sparse（USTC 已 404） | ✅ 工作流内 macOS/Windows 分别写入 TUNA sparse 配置 |
| #3 | PowerShell 不能用 `mkdir -p` | ✅ 改用 `New-Item -ItemType Directory -Force` |
| #5 | 磁贴图标模糊（尺寸不足） | ✅ `icon.ico` 七种尺寸、逐尺寸矢量渲染 |
| #6 | 区分" Partner Center 图片"与"包内磁贴" | ✅ 两套都产出，见第 5、6 节 |
| #7 | 改名后同步全部图标格式 | ✅ svg / png / ico / icns / 磁贴 全部一致 |
| #13 | 付费应用描述**零 "free"** | ✅ 全文用 "one-time purchase"，无字面 "free"（2026-09-17 起也不再出现 "7 days"，与「不启用试用」一致） |
| #14 | 描述中禁止推广其他平台 | ✅ 描述内无 macOS / Mac / Apple / Linux 字样 |
| #15 | 隐私政策链接必须可访问 | ✅ 实测 200 |
| #16 | 关键词禁止含平台/产品名 | ✅ 关键词全为功能词，无 MacOS / AutoCAD |
| #17 | 磁贴图标不能是默认图 | ✅ 300×300 商店磁贴 + 包内磁贴均为自定义 |

---

## 11. 法律与开源声明（已随应用分发）

| 项 | 位置 | 状态 |
|---|---|---|
| GPL-3.0 许可证全文 | 仓库根 `LICENSE`；构建前经 `scripts/sync-legal.mjs` 同步到 `src-tauri/resources/legal/`，随包落在 `resources/legal/LICENSE`（macOS / MSI / MSIX **同一路径**） | ✅ |
| 第三方声明（LibreDWG 归属 + 商标免责） | 仓库根 `THIRD-PARTY-NOTICES.md`；同上，随包落在 `resources/legal/THIRD-PARTY-NOTICES.md` | ✅ |
| 商标免责声明（界面可见） | 「关于」弹窗底部，中英双语 | ✅ |
| 源码 / 第三方声明链接 | 「关于」弹窗，经 `open_url` 交给系统浏览器 | ✅ |
| `license` 字段 | `package.json` 与 `src-tauri/Cargo.toml` 均为 `GPL-3.0-or-later` | ✅ |

> **商店描述里刻意不写商标免责声明。** 免责声明必然要写出 Autodesk 商标，而 10.1.3 规则对"其他产品名"敏感；
> 放在**应用界面 + 仓库**里既满足法律要求，又不把竞品名带进商店文案。官网页脚建议补同一句话（对外声明的主战场在官网）。

### 为什么随包必须有

GPL-3.0 要求向接收者提供**许可证副本**；同时 LibreDWG 以 `GPL-3.0-or-later` 分发，我们随包分发它的二进制（`dwg2dxf`），
必须保留其许可与版权声明、并提供源码获取方式。`THIRD-PARTY-NOTICES.md` 里写明了上游地址与固定版本号。

> 注：声明放在**授权内**而不是商店文案，是刻意的——审核看的是商店列表，用户看的是应用内。

### Windows 打包的坑：`../` 资源会被静默丢弃（v0.1.3 踩过）

`tauri-windows-bundle` **只收集 `resources/**`**。而 Tauri 对 `bundle.resources` 里的 `"../LICENSE"` 会映射成一个 `_up_/` 目录——macOS 打包器认这个约定（落在 `Contents/Resources/_up_/LICENSE`），**Windows 侧不报错、不警告，直接不打包**。

后果很严重：**v0.1.3 的 MSIX 会重新分发 LibreDWG 却不带它的许可证**，而 GPL-3.0 明确要求许可证副本随程序分发。**所以 v0.1.3 的包不可提交商店。**

修法（`e434b4d`，v0.1.4 起）：`scripts/sync-legal.mjs` 在 `beforeBuildCommand` 里把根目录两份文件拷进 `src-tauri/resources/legal/`；`bundle.resources` 声明 `resources/legal/*`；`.gitignore` 忽略生成物，**根目录文件仍是唯一真源**（不会漂移）。

> **验证方法：别只看仓库，拆开真实安装包。**
> `unzip -l <msix> | grep resources/legal`
> **「已推送」不等于「已随包分发」**——这条对家族里所有随包分发 GPL 二进制的软件都适用。
