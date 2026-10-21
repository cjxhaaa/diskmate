# 发布与推广清单

## 公开仓库之前

- [ ] **提交记录里的邮箱**：所有提交的作者邮箱都是工作邮箱。公开前把历史改写成 GitHub 的 noreply 邮箱
      （`<id>+cjxhaaa@users.noreply.github.com`），或者接受它公开。
- [ ] 确认 `src/mock.ts`、截图里没有真实的个人信息（已换成 `alex`、`wxid_demo_2f1a`、`game-engine` 这类示例值）。
- [ ] Settings → General → Social preview：上传 [`docs/social-preview.png`](social-preview.png)（1280×640）。
- [ ] 仓库改为公开：`gh repo edit cjxhaaa/diskmate --visibility public --accept-visibility-change-consequences`
- [ ] README 顶部徽章在公开后才会正常显示（shields.io 读不到私有仓库）。

## 发布渠道（按投入产出排序）

| 渠道 | 怎么发 | 备注 |
|---|---|---|
| **V2EX** · 分享创造 | 发帖，附 2–3 张截图 + 下载链接 | 开发者多，正好是「开发者的 C 盘」的目标用户 |
| **LINUX DO** | 发帖，截图用仓库里的图片链接 | 开发者和折腾党多；README「致谢」里已加 LINUX DO 友情链接 |
| **少数派** · Matrix 社区 | 写一篇「我为什么做了一个 C 盘清理工具」 | 偏实用，重故事和使用场景，图要多 |
| **知乎** | 回答「C 盘满了怎么清理」「C 盘突然变红」这类高浏览问题，顺带介绍 | 长尾流量最大，回答要先真解决问题 |
| **B 站 / 小红书** | 1–2 分钟录屏：扫描 → 看清大头 → 搬家腾出 40 GB | 「腾出 xx GB」的前后对比最有说服力 |
| **Hacker News** · Show HN | 见下方英文草稿，工作日北京时间晚上 9–11 点（美东上午）发 | 必须英文界面或至少英文 README；当前界面是中文，可先等英文界面 |
| **Reddit** | r/software、r/Windows11、r/opensource | 各版规不同，r/Windows 不允许自荐，先读版规 |
| **AlternativeTo** | 登记为 WizTree / TreeSize / CCleaner 的替代品 | 一次登记，持续有人搜到 |
| **Scoop** | 仓库自带 bucket（`bucket/diskmate.json`），每次发版更新版本号和哈希；稳定后可申请进 Scoop 官方 extras 仓库 | 已就绪 |
| **winget** | 向 microsoft/winget-pkgs 提交安装清单（可用 `wingetcreate new <安装包地址>` 生成） | 需要公开仓库和稳定的下载地址 |

## 文案草稿

### V2EX（分享创造）

> **标题**：做了一个 Windows C 盘分析清理工具 DiskMate，开源免费，2.6 MB，不常驻
>
> 500 GB 的 C 盘还是天天飘红，市面上的清理软件要么只删临时文件，要么一键加速不告诉你删了啥，还常驻后台弹广告。于是自己写了一个：
>
> - 先算清楚每个 GB 去了哪（OneDrive 占位文件、稀疏文件、硬链接都按实际占用算，C 盘 200 万个文件 15–20 秒扫完）
> - 每个可清理项都写明是什么、删了有什么影响，默认只勾安全的
> - 对开发者友好：Gradle、Android SDK / 模拟器、node_modules、Rust target、闲置 git worktree、WSL / Docker 的 vhdx
> - 大文件夹（微信聊天文件、SDK、缓存）一键搬到 D 盘，原路径留链接，软件不用改设置，可以一键搬回
> - 顺手做了开机启动管理、仿任务管理器的性能页、点一下窗口就能找到程序文件的「窗口定位」
> - 关窗口留托盘只占 42 MB，后台工作全是最低优先级
>
> Tauri 2 + Rust + React，MIT 协议。
> GitHub：https://github.com/cjxhaaa/diskmate
>
> （截图：概览、智能清理、搬家）
>
> 欢迎试用和提意见，尤其是「你的 C 盘里还有什么大头是它没认出来的」。

### LINUX DO

> **标题**：【开源】DiskMate：看清 C 盘都去哪了，再放心地腾出空间（Windows · 2.6 MB · 不常驻）
>
> ---
>
> 各位佬友好，分享一个自己写的 Windows 磁盘分析清理工具 **DiskMate**，开源免费（MIT）。
>
> 起因很简单：C 盘隔三差五就飘红，市面上的清理软件要么只删删临时文件，要么「一键加速」却不告诉你删了什么，还要常驻后台弹窗。我想要的是先把账算清楚，再自己决定删什么，于是就自己写了一个。
>
> ![DiskMate](https://raw.githubusercontent.com/cjxhaaa/diskmate/master/docs/social-preview.png)
>
> ## 能做什么
>
> - **概览**：按类别分账，系统、软件、开发工具与缓存、应用数据、个人文件、虚拟内存 / 休眠…… 每个 GB 去了哪一目了然。OneDrive 占位文件、稀疏 / 压缩文件、硬链接都按实际占用算，不会算出比磁盘还大的数。
> - **智能清理**：系统垃圾、开发缓存、浏览器缓存、旧版 IDE 数据、闲置的 Git worktree 等，每一项都写清楚是什么、删了有什么影响，分「安全 / 谨慎 / 需确认」三档，**默认只勾安全项**。
> - **搬家与压缩**：把微信聊天文件、Android SDK、Gradle 缓存、大项目文件夹搬到 D 盘，原路径留下链接，**软件不用改任何设置**；搬过的都列在「已经搬走的」里，随时一键搬回 C 盘。还能压缩只涨不缩的 WSL / Docker `.vhdx`。
> - **应用管理**：每个软件的程序大小 + 它藏在 AppData 里的数据大小 + 上次使用时间，卸载后顺手清残留。
> - **空间分析 / 空间变化**：矩形树图逐层点进去；对比历史扫描，看最近是谁在长。
> - **文件整理**：下载文件夹里装过的安装包、解压过的压缩包、重复文件、超大文件，默认进回收站。
> - **开机启动**：列出所有启动项，给出「可以关 / 建议保留」和理由，开关和任务管理器是同一个设置。
> - **性能**：仿任务管理器的实时曲线（CPU 每个核心、内存、磁盘、网络、GPU）。
> - **窗口定位**：点一下屏幕上任意窗口，就知道它是哪个程序、文件在哪、命令行是什么。
>
> ![智能清理](https://raw.githubusercontent.com/cjxhaaa/diskmate/master/docs/screenshots/cleanup-light.webp)
>
> ![搬家](https://raw.githubusercontent.com/cjxhaaa/diskmate/master/docs/screenshots/move-light.webp)
>
> ## 对开发者比较友好
>
> C 盘真正的大头往往不是系统垃圾，而是这些：Gradle、Android SDK 与模拟器镜像、node_modules、Rust `target`、各种包管理器缓存、闲置的 Git worktree、只涨不缩的 WSL / Docker 虚拟磁盘。DiskMate 会把它们单独列出来，能清的清，清不了的就搬。
>
> ## 关于「放心」
>
> 清理工具最怕删错，所以做了这些限制：
>
> - 界面只能选后端分析出来的项目，不能传任意路径去删；每个路径都要过白名单。
> - 能进回收站的都进回收站；微信、QQ 聊天文件只给指引，不批量删。
> - 搬家是「完整复制 → 逐个核对 → 原位置换成链接 → 删除原件」，中途任何一步出错都会自动撤销，原文件不动。
> - 平时不以管理员身份运行，需要管理员的操作（压缩虚拟磁盘、改「所有用户」的启动项）单独弹授权。
>
> ## 关于「轻」
>
> - 安装包 2.6 MB，装在当前用户下，不需要管理员权限。
> - 关窗口可以留在托盘，只占约 42 MB 内存；不加开机启动，没有后台服务和定时任务。
> - 扫描只用 2–6 个线程，并且处于 Windows 后台模式（CPU / 磁盘 / 内存优先级都最低）。实测 NTFS 遍历过了约 4 个线程就不再变快，多开线程只是白烧 CPU。
> - 一块 200 多万个文件的系统盘，扫一遍大约 15–20 秒。
>
> ## 下载
>
> - Release（安装版 / 便携版）：https://github.com/cjxhaaa/diskmate/releases/latest
> - Scoop：
>   ```
>   scoop bucket add diskmate https://github.com/cjxhaaa/diskmate
>   scoop install diskmate
>   ```
> - 一分钟介绍视频和实操演示：https://github.com/cjxhaaa/diskmate#介绍视频
>
> 支持 Windows 10（1809+）/ 11，64 位。技术栈是 Tauri 2 + Rust + React，源码：https://github.com/cjxhaaa/diskmate
>
> ## 已知问题
>
> - 界面目前只有简体中文，英文界面在路线图里。
> - 安装包还没有代码签名，SmartScreen 可能拦一下，点「更多信息 → 仍要运行」即可。
>
> 欢迎试用和拍砖，有问题或想要的功能可以直接回帖，或者到 GitHub 提 Issue。觉得有用的话点个 Star 就很感谢了 🙏

### Show HN

> **Title**: Show HN: DiskMate – a Windows disk cleaner that explains every byte it deletes
>
> I kept running out of space on a 500 GB C: drive, and the usual cleaners either only emptied temp folders or "optimized" things without saying what they removed. DiskMate measures where space actually goes (handling OneDrive placeholders, sparse files and hard links), explains each cleanup item and its consequences, and only pre-selects the safe ones.
>
> A few things I found interesting while building it:
> - Directory walking on NTFS stops scaling at ~4 threads; 20 threads used 40–80% more CPU for the same wall time, so the scanner now uses a small background-priority pool.
> - Moving big folders to another drive works by copy → verify → replace with a junction, so apps never notice; known folders (Downloads, Documents) use SHSetKnownFolderPath instead.
> - The startup manager uses the same `StartupApproved` flags as Task Manager, so nothing is deleted and both stay in sync.
>
> It's Tauri 2 (Rust + React), 2.6 MB installer, MIT. The UI is Chinese for now; English is next.
> https://github.com/cjxhaaa/diskmate

### 短视频脚本（60–90 秒）

1. 开场（5 秒）：C 盘飘红的截图 + 「500 GB 的 C 盘到底被谁吃了？」
2. 扫描（10 秒）：打开 DiskMate，15 秒扫完，环形图展开。
3. 看清大头（20 秒）：鼠标移到各类别上；空间分析点进 AppData 找到微信 30 GB。
4. 清理（15 秒）：智能清理，每项有说明，默认只勾安全的，清完显示释放 9 GB。
5. 搬家（20 秒）：勾选微信聊天文件、Android SDK，顶部显示「C 盘可用 78 → 124 GB」，搬完微信照常打开。
6. 结尾（5 秒）：开源免费、2.6 MB、GitHub 地址。

## 发布节奏建议

1. 先公开仓库、发 v0.1.0，自己和身边朋友用一周，收集第一批问题。
2. 修完明显问题发 v0.1.1，然后同一周内发 V2EX + 少数派 + 知乎回答。
3. 做好英文界面后再发 Show HN 和 Reddit。
4. 每次发版在 CHANGELOG 写清楚，Release 页面附上截图。
