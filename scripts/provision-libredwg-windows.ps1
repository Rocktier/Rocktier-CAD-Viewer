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
#   pwsh -File scripts/provision-libredwg-windows.ps1 -Download -Version 0.13.3
#
# 注：LibreDWG 的 Windows 发行包目录是扁平的（dwg2dxf.exe 与 DLL 同在根目录）。
param(
  [string]$Source = "",
  [string]$OutDir = "",
  [switch]$Download,
  [string]$Version = "0.13.3"
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

# 定位 LibreDWG win64 发行包目录（兼容解压后多一层子目录的情况）
function Find-LibreDwgRoot {
  if ($Source -and (Test-Path "$Source\dwg2dxf.exe")) { return $Source }
  if ($Source) {
    $hit = Get-ChildItem -Path $Source -Filter "dwg2dxf.exe" -Recurse -ErrorAction SilentlyContinue |
           Select-Object -First 1
    if ($hit) { return $hit.DirectoryName }
  }
  foreach ($c in @("F:\deps\libredwg", "$env:LOCALAPPDATA\libredwg", "$env:USERPROFILE\libredwg")) {
    if (Test-Path "$c\dwg2dxf.exe") { return $c }
  }
  throw "未找到 LibreDWG win64 发行包。请用 -Download 自动获取，或从 https://github.com/LibreDWG/libredwg/releases 下载 libredwg-*-win64.zip 解压后用 -Source 指定目录。"
}

$Root = Find-LibreDwgRoot
Write-Host "LibreDWG 根目录: $Root"

if (Test-Path $OutDir) { Remove-Item -Recurse -Force $OutDir }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# dwg2dxf.exe 只直接依赖 libredwg-0.dll（dumpbin /DEPENDENTS 验证）；
# 旧版发行包可能还需要 libiconv / pcre2，存在则一并带上。
Copy-Item "$Root\dwg2dxf.exe"     "$OutDir\dwg2dxf.exe"
Copy-Item "$Root\libredwg-0.dll"  "$OutDir\libredwg-0.dll"
foreach ($d in @("libiconv-2.dll", "libpcre2-16-0.dll", "libpcre2-8-0.dll")) {
  if (Test-Path "$Root\$d") { Copy-Item "$Root\$d" "$OutDir\$d" }
}

# 冒烟测试：确保 sidecar 可运行
& "$OutDir\dwg2dxf.exe" --version | ForEach-Object { Write-Host "dwg2dxf: $_" }
if ($LASTEXITCODE -ne 0) { throw "dwg2dxf.exe 无法运行（缺少依赖 DLL？）" }

# 目录本身要入库（.gitkeep），否则 macOS 构建会因 resources 缺失而失败 ——
# 上面的 Remove-Item 会把它一起删掉，这里补回来。
New-Item -ItemType File -Force -Path "$OutDir\.gitkeep" | Out-Null

$size = ((Get-ChildItem -Recurse $OutDir | Measure-Object Length -Sum).Sum / 1MB)
Write-Host "✅ LibreDWG sidecar 就绪: $OutDir ($([math]::Round($size,1))MB)"
