# 分流岛 · RouteIsle

分流岛是一个按域名选择网络出口的桌面工具。把公司内网交给 Windows 选路，把指定网站交给物理网卡，其余流量转给本机上游代理。桌面界面采用可展开的胶囊布局，方便查看状态和编辑规则。

## 当前状态

| 部分 | 当前能力 |
| --- | --- |
| Windows 分流脚本 | 启动本地 HTTP/SOCKS 代理，检查飞鸟上游，接管和恢复系统代理及指定 Gitea 域名的 Git 代理 |
| Vue 界面 | 编辑和启停域名规则组，配置 HTTP/SOCKS5 上游、直连和拒绝连接，支持深浅主题 |
| Tauri 桌面预览 | 透明无边框置顶胶囊，点击展开，拖动后可吸附四边、四角和边中点 |
| 运行详情 | 模拟实时速率、60 秒趋势、按出口和规则组统计、运行历史与连接日志 |

界面的运行和 TUN 开关使用模拟数据。编辑的规则保存在当前会话，最近 30 次运行的统计和日志保存在本地。真实分流可通过下文的 Windows 脚本运行，脚本读取自己的规则文件或命令行参数。

## 预览界面

在项目目录安装依赖并启动 Vite：

```powershell
npm install
npm run dev
```

打开终端显示的地址，默认是 http://127.0.0.1:1420/。点击胶囊展开规则面板。界面上的启动和停止按钮只改变模拟状态。

`npm run build` 构建前端。Windows 上安装 Rust（含 Cargo）、Visual Studio C++ Build Tools、Windows SDK 和 WebView2 后，可运行桌面预览。

```powershell
npm run desktop
```

启动后，胶囊出现在屏幕顶部。单击展开或收起，按住拖动可改变位置。运行时显示上传和下载速率，未运行时显示「未启用」。展开「运行详情」后，可悬停趋势图查看采样，或按日期筛选历史与日志。

## 运行 Windows 分流脚本

当前脚本以飞鸟为上游，要求飞鸟已连接并监听 `127.0.0.1:7890`。关闭飞鸟的「系统代理」开关，让分流岛接管该设置。脚本默认加载项目目录中的 `ApexCore.exe`，也可以在本地规则文件中指定内核路径。

1. 双击 [Start-Proxy.cmd](Start-Proxy.cmd)。启动器验证飞鸟以及经本地分流访问内外网的连通性，通过后将 Windows 系统代理切到 `127.0.0.1:17890`。
2. 保持启动窗口打开。遵循系统代理的应用会使用分流规则。
3. 按 `Ctrl+C` 停止并恢复代理。异常退出后，可双击 [Restore-Proxy.cmd](Restore-Proxy.cmd) 恢复 Windows 和 Git 的代理设置。

启动检查默认访问 `www.example.com`。可在本地规则文件中添加一个内网检查地址，并指定需要联动代理的 Git 主机。

### 配置域名规则

在项目根目录创建本地文件 `proxy-rules.json`。两个数组都按域名后缀匹配，包含子域名。重启分流脚本后生效。

```json
{
  "SystemRoute": ["intranet.example.com"],
  "PhysicalDirect": ["direct.example.com"],
  "Core": "C:/path/to/ApexCore.exe",
  "HealthCheckUrl": "https://git.example.com/",
  "GitProxyHost": "git.example.com"
}
```

`Core`、`HealthCheckUrl` 和 `GitProxyHost` 可按需填写。`proxy-rules.json` 由 Git 忽略。

- `SystemRoute` 解析后由 Windows 选路。目标 IP 命中现有的 WireGuard 路由时，流量进入隧道。
- `PhysicalDirect` 尝试通过指定网卡连接，默认网卡名为 `WLAN`。实际公网出口仍可能受系统的 TUN 路由影响。
- 其余请求交给飞鸟的 `7890` 端口。脚本的指定域名使用经飞鸟转发的 DoH 解析。

## 技术栈

Tauri 2、Vue 3、Vite、UnoCSS、Reka UI、Lucide，以及用于 Windows 代理管理的 PowerShell 脚本。
