# DiskMate

Windows 磁盘空间分析与清理工具。参考 WizTree / TreeSize（空间分析）、CCleaner / 电脑管家（分类清理）和
npkill / kondo（开发者构建产物）的做法，针对「开发者的 C 盘」做了专门的规则。

## 功能

- **概览**：磁盘用量 + 按类别的环形图（系统、软件、开发工具与缓存、应用数据、个人文件、项目、虚拟内存/休眠、
  系统保留区），鼠标移上去解释每一类是什么。
- **智能清理**：分组列出可清理的内容，每项都写明是什么、删了有什么影响，并标注「安全 / 谨慎 / 需确认」。
  默认只勾选安全项；清理后显示实际释放的空间和可用空间变化。
  - 系统垃圾：临时文件（只删 1 天前的）、Windows 更新缓存、崩溃转储、着色器缓存、回收站
  - 开发缓存：Gradle 日志/依赖、npm/bun/pip/yarn/Go 缓存、cargo 缓存、旧版 Gradle、没被模拟器使用的
    Android 系统镜像、模拟器快照、旧版本 IDE 缓存、VS Code/Cursor 缓存、AI 插件索引、
    30 天未改动项目的 node_modules / target / build
  - 应用缓存：Chrome / Edge 缓存（不动书签、密码、登录状态）
  - 需要确认：闲置的 Git worktree（`git worktree remove`，分支保留）、60 天没用的模拟器、关闭休眠
  - 只给指引：微信、QQ 音乐、Claude、下载文件夹、虚拟内存——这些直接删文件夹会出问题，只告诉你在哪清
- **应用管理**：列出桌面程序（注册表 64/32 位视图和当前用户安装的）和商店应用，显示实测的程序大小、
  它在 AppData 等处存的数据大小、上次使用时间（系统的启动记录 UserAssist + 数据文件夹的活动时间）。
  超过 3 个月没用的大应用会被标成「建议卸载」，运行库和驱动会提示不要随便卸。卸载时运行应用自带的卸载程序
  （MSI 用 msiexec，商店应用用系统自带的移除命令），等它从已安装列表里消失后，检查并提供清理用户目录里的残留数据。
- **空间分析**：可逐层点进去的矩形树图 + 按大小排序的列表。
- **文件整理**：
  - 下载文件夹：找出已经装过的软件的安装包、已经解压过的压缩包、3 个月没动的大文件；
  - 重复文件：在用户目录里按「大小 → 头尾 64 KB → 完整 BLAKE3」三步比对，跳过 AppData、工具目录、
    代码仓库和 OneDrive 云端文件；「智能选择」每组保留一份，微信 / QQ 聊天文件受保护；
  - 大文件：超过 100 MB 的文件，按类型筛选。
  删除默认进回收站，也可以选择直接删除。
- **搬家与压缩**：
  - 搬到其他盘：Gradle 缓存、Android SDK / 模拟器、JetBrains 缓存、微信聊天文件、QQ 音乐、用户目录下的大项目文件夹。
    用 robocopy 完整复制并逐个核对后，原位置换成目录联接（junction），软件照原路径访问；下载、文档、图片、视频、
    音乐用 Windows 的「位置」机制（SHSetKnownFolderPath）改。搬之前检查相关程序是否在运行、先试探文件夹是否被占用，
    出错自动撤销；每次搬家都有记录，可以一键搬回。
  - 压缩虚拟磁盘：找出 WSL（注册表 Lxss）、Docker、Claude 和扫描到的其他 .vhdx，WSL 先在发行版里 fstrim 再
    `wsl --shutdown`，然后用 diskpart（管理员）compact。
- **空间变化**：每次扫描保存一份快照（总量、各类别、所有 50 MB 以上的文件夹，每个盘保留 30 次），
  画出已用空间的趋势，和任意一次历史扫描对比哪些文件夹变大 / 变小。启动时如果 24 小时内扫描过，直接显示上次结果。

## 安全设计

- 前端只能按 id 选择后端分析出来的项目，不能传任意路径去删。
- 每个要删除的路径都要通过白名单检查（`clean::allowed`）：必须在用户目录或几个已知的系统缓存目录之内，
  且不能是用户目录、AppData、文档、桌面、下载这些根目录本身。
- 所有文件操作走 `\\?\` 长路径，不会因为路径超过 260 字符删一半。
- 正在使用的文件跳过不报错；释放的空间按实际删掉的文件累计。
- 大文件页不允许删除 AppData 和 `.android` / `.gradle` 这类工具目录里的单个文件。

## 开发

```bash
npm install
npm run tauri dev      # 开发模式
npm run tauri build    # 生成 src-tauri/target/release/bundle/nsis/DiskMate_*_x64-setup.exe
```

只看界面不需要 Rust：`npm run dev` 后在浏览器打开 http://127.0.0.1:1430，会用 `src/mock.ts` 的假数据。

测试：

```bash
cd src-tauri
cargo test                                         # 单元测试
cargo test --release real_scan -- --ignored --nocapture      # 只读：扫描本机系统盘
cargo test --release real_analysis -- --ignored --nocapture  # 只读：分析本机可清理内容
```

## 已知限制

- 扫描用普通权限，`System Volume Information` 等目录读不到，计入「系统保留区」。
- 只对 8 MB 以上的硬链接文件去重；`C:\Windows\WinSxS` 里的小文件硬链接会被重复计算，
  所以「Windows 系统」一项会偏大一些。
