# Rocktier CAD Viewer - 打包 LibreDWG sidecar (Windows)
#
# 从 LibreDWG 官方 Windows 发行包复制 dwg2dxf.exe 及其依赖 DLL 到
# src-tauri/resources/libredwg/。tauri.conf.json 已声明该目录为 bundle
# resources，NSIS/MSI/MSIX 安装时会放到 <安装目录>/resources/libredwg/，
# Rust 端 cli_bin() 会在此处找到 dwg2dxf。
#
# 用法:
#   # 本地：先手动下载 libredwg-*-win64.zip 并解压，再用 -Source 指向该目录
#   pwsh -File scripts/provision-libredwg-windows.ps1 -Source F:\deps\libredwg
#
#   # CI：自动下载官方发行包（无需预先准备）
#   pwsh -File scripts/provision-libredwg-windows.ps1 -Download
#   pwsh -File scripts/provision-libredwg-windows.ps1 -Download -Version 0.14.8597
#
# 版本选择：**必须与 macOS 端（Homebrew libredwg）保持同一大版本**。
# 0.13.3 能写出 LAYER 表却解不出图形（表现为图层列得出来、主画面空白），
# 0.14.x 才与 macOS 行为一致。0.14.x 目前是 GitHub 上的 prerelease，这是
# 刻意的选择——最后一个非 prerelease（0.13.3）解析能力不足。
#
# 注：发行包目录布局可能变化，下面所有文件都用递归查找定位，不写死层级。
param(
  [string]$Source = "",
  [string]$OutDir = "",
  [switch]$Download,
  [string]$Version = "0.14.8597"
)
$ErrorActionPreference = "Stop"
$ProjectRoot = (Resolve-Path "$PSScriptRoot/..").Path
if (-not $OutDir) { $OutDir = "$ProjectRoot/src-tauri/resources/libredwg" }

# CI 用：自动下载并解压官方 win64 发行包
if ($Download) {
  $zip   = Join-Path $env:TEMP "libredwg-$Version-win64.zip"
  $dest  = Join-Path $env:TEMP "libredwg-$Version-win64"
  $url   = "https://github.com/LibreDWG/libredwg/releases/download/$Version/libredwg-$Version-win64.zip"
  Write-Host "下载 LibreDWG $Version ..."
  Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
  if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
  New-Item -ItemType Directory -Force -Path $dest | Out-Null
  Expand-Archive -Path $zip -DestinationPath $dest -Force
  $Source = $dest
}

# 递归找一个文件，找到即返回其完整路径（发行包层级变化也不怕）
function Find-In([string]$Tree, [string]$Name) {
  if (-not $Tree) { return $null }
  $hit = Get-ChildItem -Path $Tree -Filter $Name -Recurse -File -ErrorAction SilentlyContinue |
         Select-Object -First 1
  if ($hit) { return $hit.FullName }
  return $null
}

# 定位 dwg2dxf.exe：-Source 指定的目录优先，其次常见本地路径
$dwg2dxf = $null
if ($Source) { $dwg2dxf = Find-In $Source "dwg2dxf.exe" }
if (-not $dwg2dxf) {
  foreach ($c in @("F:\deps\libredwg", "$env:LOCALAPPDATA\libredwg", "$env:USERPROFILE\libredwg")) {
    $dwg2dxf = Find-In $c "dwg2dxf.exe"
    if ($dwg2dxf) { $Source = $c; break }
  }
}
if (-not $dwg2dxf) {
  throw "未找到 LibreDWG win64 发行包。请用 -Download 自动获取，或从 https://github.com/LibreDWG/libredwg/releases 下载 libredwg-*-win64.zip 解压后用 -Source 指定目录。"
}
$Root = Split-Path $dwg2dxf -Parent
Write-Host "LibreDWG 根目录: $Root"
Write-Host "宿源目录: $(if ($Source) { $Source } else { $Root })"

if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# dwg2dxf.exe 只直接依赖 libredwg-0.dll（dumpbin /DEPENDENTS 验证）；
# 旧版发行包可能还需要 libiconv / pcre2，存在则一并带上。
# 每个文件都在整棵解压树里找，避免发行包换层级后复制失败。
function Copy-Needed([string]$Pattern, [switch]$Required) {
  $p = Find-In $Source $Pattern
  if (-not $p) { $p = Find-In $Root $Pattern }
  if ($p) {
    Copy-Item $p (Join-Path $OutDir (Split-Path $p -Leaf))
    Write-Host "  + $(Split-Path $p -Leaf)  <- $p"
    return
  }
  if ($Required) { throw "缺少必需文件: $Pattern（发行包结构或命名可能变了）" }
  Write-Host "  - $Pattern（不存在，跳过）"
}

Write-Host "复制 sidecar 文件:"
Copy-Needed "dwg2dxf.exe"   -Required
Copy-Needed "libredwg*.dll" -Required
foreach ($d in @("libiconv*.dll", "libpcre2*.dll")) { Copy-Needed $d }

# 冒烟测试：确保 sidecar 可运行，并打印版本以便和 macOS 端对账
& "$OutDir\dwg2dxf.exe" --version | ForEach-Object { Write-Host "dwg2dxf: $_" }
if ($LASTEXITCODE -ne 0) { throw "dwg2dxf.exe 无法运行（缺少依赖 DLL？）" }

# 目录本身要入库（.gitkeep），否则 macOS 构建会因 resources 缺失而失败 ——
# 上面的 Remove-Item 会把它一起删掉，这里补回来。
New-Item -ItemType File -Force -Path "$OutDir\.gitkeep" | Out-Null

$size = ((Get-ChildItem -Recurse $OutDir | Measure-Object Length -Sum).Sum / 1MB)
Write-Host "✅ LibreDWG sidecar 就绪: $OutDir ($([math]::Round($size,1))MB)"
