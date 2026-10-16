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
