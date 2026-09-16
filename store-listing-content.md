# Microsoft Store Listing — Rocktier CAD Viewer

> 照 `Rocktier-Windows-Store-上架避坑指南.md` 逐条规避；本文末尾有对照自查表。

---

## 1. 应用信息

| 字段 | 内容 |
|------|------|
| **应用名称** | Rocktier CAD Viewer |
| **发布者显示名称** | Rocktier |
| **包标识（Identity Name）** | `Rocktier.CADViewer`（在 Partner Center 创建产品时确定，须与 MSIX 一致） |
| **Publisher** | `CN=4EA39D7A-401B-4D56-98D0-8ECB1F2B8DF7`（与 PDF Squeeze 同一 Partner Center 账号，已写入 `src-tauri/gen/windows/bundle.config.json`） |
| **定价** | US$4.99 买断（一次性） |
| **试用** | 7 天限时试用（Time-limited trial） |
| **隐私政策 URL** | `https://rocktier.com/privacy` |
| **支持联系邮箱** | hello@rocktier.com |
| **支持 URL** | `https://rocktier.com/` |

---

## 2. English listing

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
>
> **Built for**
>
> Checking a drawing a client just sent. Confirming a dimension before you quote. Showing a plan in a meeting without booting a full CAD workstation.
>
> This is a viewer, on purpose: it reads drawings, measures them and inspects them, and it never writes to your originals.
>
> English and Chinese (Simplified) interface.
>
> Try it for 7 days. After that it is a one-time purchase — no subscription, no renewal.

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

## 9. 踩坑对照自查

| 踩坑 | 内容 | 本项目处理 |
|------|------|-----------|
| #1 | cargo 镜像用 TUNA sparse（USTC 已 404） | ✅ 工作流内 macOS/Windows 分别写入 TUNA sparse 配置 |
| #3 | PowerShell 不能用 `mkdir -p` | ✅ 改用 `New-Item -ItemType Directory -Force` |
| #5 | 磁贴图标模糊（尺寸不足） | ✅ `icon.ico` 七种尺寸、逐尺寸矢量渲染 |
| #6 | 区分" Partner Center 图片"与"包内磁贴" | ✅ 两套都产出，见第 5、6 节 |
| #7 | 改名后同步全部图标格式 | ✅ svg / png / ico / icns / 磁贴 全部一致 |
| #13 | 付费应用描述**零 "free"** | ✅ 全文用 "7 days"、"one-time purchase"，无 "free" |
| #14 | 描述中禁止推广其他平台 | ✅ 描述内无 macOS / Mac / Apple / Linux 字样 |
| #15 | 隐私政策链接必须可访问 | ✅ 实测 200 |
| #16 | 关键词禁止含平台/产品名 | ✅ 关键词全为功能词，无 MacOS / AutoCAD |
| #17 | 磁贴图标不能是默认图 | ✅ 300×300 商店磁贴 + 包内磁贴均为自定义 |
