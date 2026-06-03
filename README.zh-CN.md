# QuickShot

小巧、快速的截图守护进程，支持 **macOS** 与 **Windows**。纯 Rust 实现，二进制约 2-3 MB，常驻系统托盘。

[![Release](https://img.shields.io/github/v/release/simmzl/QuickShot)](https://github.com/simmzl/QuickShot/releases/latest)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#许可证)

> [English](README.md) | 简体中文

## 功能特性

- **区域截图 + 全屏截图**，全局快捷键可配置。
- **拖拽 → 锚点调整 → 确认** 流程，任意阶段可按 `Esc` 取消。
- **标注工具**：箭头、矩形、椭圆、马赛克、画笔、文字、移动 —— 支持撤销 / 重做。
- **选区下浮动工具栏**：切换工具，可将选区固定为悬浮预览，或另存为 PNG。
- **4× 放大镜**：拖拽时显示十字准线、HEX 色值 / 坐标、实时 W × H 尺寸标签。
- **系统托盘菜单**：区域截图 / 全屏截图、编辑配置、开机自启、退出。
- **PNG 自动保存**，文件名支持日期 / 时间 / 尺寸 / 模式占位符。
- **配置热重载** —— 编辑 `config.toml` 保存后 1 秒内自动生效，快捷键和托盘标签实时刷新，无需重启。

## 安装

每个 [GitHub Release](https://github.com/simmzl/QuickShot/releases/latest) 都附带预编译产物。

### macOS

下载 `QuickShot-<VERSION>.dmg`。双击 → 把 `QuickShot.app` 拖到 `Applications` → 推出 DMG。

首次启动时，Finder 会提示「来自身份不明的开发者」（对于未购买 Apple Developer ID 签名的开源应用，这是正常现象）。绕过一次即可：

1. 在 Finder 中打开 `Applications`。
2. **右键**点击 `QuickShot.app` → **打开** → 在确认对话框中点 **打开**。

macOS 会记住这次授权；以后启动（包括开机自启）都不会再弹这个提示。

首次截图时，macOS 会请求**屏幕录制**权限。在 **系统设置 → 隐私与安全性 → 屏幕录制** 中启用 `QuickShot`，然后重新启动应用。

### Windows

下载 `QuickShot-<VERSION>-windows-x64.zip`。解压到任意目录，双击 `QuickShot.exe`。程序会常驻系统托盘 —— 不会弹出黑色控制台窗口。

首次运行时，SmartScreen 可能提示 *"Windows 已保护你的电脑"*。点击 **更多信息 → 仍要运行** 一次即可，系统会记住这次授权。

## 使用

默认快捷键：

| 操作 | macOS | Windows |
|---|---|---|
| 区域截图 | `Cmd+Shift+A` | `Ctrl+Shift+A` |
| 全屏截图 | `Cmd+Shift+S` | `Ctrl+Shift+S` |

截图成功后会自动复制到剪贴板，也可选择同时保存为 PNG（见 [配置](#配置)）。

### 区域截图 —— 调整状态

初次拖拽完毕后，选区进入「调整状态」：可以拖动边缘改变尺寸，也可以添加标注。

| 按键 | 操作 |
|---|---|
| `Enter` 或双击 | 确认 —— 复制到剪贴板 |
| `Esc` | 取消 |
| `A` / `R` / `E` / `B` | 箭头 / 矩形 / 椭圆 / 马赛克 |
| `P` / `T` | 画笔 / 文字 |
| `M` | 移动已有标注 |
| `Cmd/Ctrl + Z` | 撤销 |
| `Cmd/Ctrl + Shift + Z` | 重做 |

选区下方的工具栏映射上述工具，并额外提供：

- **固定 (Pin)** —— 将裁剪后的图像变成始终置顶的悬浮预览窗口。可同时存在多个 pin；拖动可移动位置，双击关闭。
- **另存为…** —— 弹出系统保存对话框，将裁剪 + 标注后的图像保存到你指定的路径。

### 托盘菜单

右键点击托盘图标：**区域截图**、**全屏截图**、**编辑配置…**、**开机自启**、**退出**。

## 配置

配置文件路径：

- macOS：`~/.config/QuickShot/config.toml`
- Windows：`%APPDATA%\QuickShot\config.toml`

首次运行会写入默认配置。**编辑后 1 秒内自动重新加载** —— QuickShot 会轮询文件的 mtime，发现变更时实时重新绑定快捷键、更新托盘菜单标签、刷新保存 / 通知设置，无需重启。

```toml
[hotkey]
# 格式：用 "+" 连接修饰键，最后是按键。修饰键（大小写不敏感）：
#   Cmd / Meta / Super（在 Windows 上即 Win 键）/ Ctrl / Alt / Opt / Shift
# 按键：A-Z, 0-9, F1-F24，或命名键（Space, Enter, Tab, Backspace, Escape）。
region = "Cmd+Shift+A"          # Windows 默认值："Ctrl+Shift+A"
fullscreen = "Cmd+Shift+S"      # Windows 默认值："Ctrl+Shift+S"

[save]
# 设为 true 时，每次成功截图会同时写一份 PNG 到 `directory`。
enabled = false
# `~` 在 macOS 上展开为 $HOME，在 Windows 上展开为 %USERPROFILE%。目录不存在会自动创建。
directory = "~/Desktop"         # Windows 默认值："~/Pictures"
# 占位符：{date} {time} {datetime} {w} {h} {mode}
filename_template = "Screenshot_{datetime}.png"

[general]
# 全屏截图成功后，是否弹出系统通知。
notification_on_fullscreen = true
```

**Windows 提示**：`Win+Shift+S` 已被系统自带的截图工具 (Snipping Tool) 占用。请避免将全屏快捷键设成 `Super+Shift+S` 或 `Meta+Shift+S` —— 注册会失败并回滚到旧绑定。建议使用 `Ctrl` 或 `Alt` 系列组合。

文件名模板占位符：

| 占位符 | 示例 |
|---|---|
| `{date}` | `2026-04-19` |
| `{time}` | `15-04-30` |
| `{datetime}` | `2026-04-19_15-04-30` |
| `{w}`、`{h}` | `1920`、`1080`（物理像素） |
| `{mode}` | `region` 或 `fullscreen` |

## 开机自启

### macOS

托盘菜单 → **开机自启 (Start at Login)**，或通过命令行：

```
/Applications/QuickShot.app/Contents/MacOS/QuickShot --install-autostart
/Applications/QuickShot.app/Contents/MacOS/QuickShot --uninstall-autostart
```

会安装 `~/Library/LaunchAgents/com.QuickShot.daemon.plist`，下次登录时生效。对于直接运行 `target/release/QuickShot` 的用户，同样的命令行参数也有效 —— plist 中会写入当前运行二进制的实际路径。

### Windows

托盘菜单 → **开机自启 (Start at Login)**。会在注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\QuickShot` 下写入一个指向当前 exe 的字符串值，下次登录时生效。可在 PowerShell 中验证或手动移除：

```powershell
# 查询
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v QuickShot

# 移除（通常从托盘菜单关闭即可）
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v QuickShot /f
```

## 从源码构建

需要 Rust stable（1.75+）。

```
cargo build --release
```

产物在 `target/release/QuickShot[.exe]`。Release 配置已针对体积优化（`opt-level="z"`、`lto=true`、`codegen-units=1`、`strip=true`、`panic="abort"`）：macOS 通用二进制约 3.2 MB，Windows 二进制约 2.1 MB。

Windows 端构建还需要 MSVC 链接器 (`link.exe`)。请安装 **Visual Studio Build Tools** 并勾选 **使用 C++ 的桌面开发** 工作负载；或者在调用 `cargo` 之前先用 `vcvars64.bat` 初始化 MSVC 环境。

### 打包

| 平台 | 脚本 | 产物 |
|---|---|---|
| macOS | `bash scripts/package.sh` | `dist/QuickShot.app`（universal x86_64 + aarch64，ad-hoc 签名）+ `dist/QuickShot-<VERSION>.dmg` |
| Windows | `pwsh scripts/package.ps1`（在 Developer PowerShell 中运行） | `dist/QuickShot-<VERSION>-windows-x64/`（exe + README）+ `dist/QuickShot-<VERSION>-windows-x64.zip` |

macOS 端 `package.sh` 支持的环境变量：

- `BUNDLE_ID`（默认 `com.QuickShot.app`）
- `SIGN_IDENTITY`（默认 `-` 即 ad-hoc 签名；传入 Apple Developer ID 字符串可做完整签名）

### Release CI

推送 `v*` 形式的 git tag 会触发 [`.github/workflows/release.yml`](.github/workflows/release.yml)：

1. `build-macos`（运行于 `macos-latest`）和 `build-windows`（运行于 `windows-latest`）并行执行，各自跑 `cargo test`、打包、上传 artifact。
2. `release` job 下载两个 artifact，发布单个 GitHub Release（同时包含 dmg 和 zip）。

发版命令：

```
# 1. 在 Cargo.toml 中升级版本号，提交。
# 2. 打 tag 并推送。
git tag v1.2.3
git push origin master v1.2.3
```

## 卸载

### macOS

```
# 如果之前启用了开机自启，先关闭：
/Applications/QuickShot.app/Contents/MacOS/QuickShot --uninstall-autostart

# 删除应用本体：
rm -rf /Applications/QuickShot.app

# 可选：删除配置 + 缓存：
rm -rf ~/.config/QuickShot
```

### Windows

1. 从托盘菜单 **退出 (Quit)**。
2. 删除解压出的文件夹。
3. （可选）如果启用过开机自启，移除注册表项：
   ```powershell
   reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v QuickShot /f
   ```
4. （可选）删除配置目录：
   ```powershell
   Remove-Item -Recurse "$env:APPDATA\QuickShot"
   ```

## 许可证

MIT OR Apache-2.0 —— 任选其一适合你的项目。
