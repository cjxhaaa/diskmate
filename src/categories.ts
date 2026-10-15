import type { CategoryId, Group, Risk } from "./api";

export interface CategoryMeta {
  label: string;
  color: string;
  hint: string;
}

export const CATEGORIES: Record<CategoryId, CategoryMeta> = {
  system: { label: "Windows 系统", color: "#6C7BFF", hint: "Windows 本体、系统更新和恢复环境。不建议手动删除。" },
  programs: { label: "已安装的软件", color: "#3FA9F5", hint: "Program Files 和 ProgramData 里的应用程序。不需要的软件请到「设置 › 应用」里卸载。" },
  vm: { label: "虚拟内存 / 休眠", color: "#A78BFA", hint: "pagefile.sys 和 hiberfil.sys。休眠文件可以在智能清理里关闭。" },
  dev: { label: "开发工具与缓存", color: "#F59E0B", hint: "Android SDK、模拟器、Gradle / npm / cargo 缓存、IDE 缓存、node_modules 等。" },
  appdata: { label: "应用数据", color: "#EC4899", hint: "各个软件在 AppData 里的数据和缓存，例如浏览器、微信、QQ 音乐、Claude。" },
  temp: { label: "临时文件", color: "#2DD4BF", hint: "临时文件夹和崩溃转储，大部分可以直接清理。" },
  personal: { label: "个人文件", color: "#22C55E", hint: "文档、桌面、下载、图片、视频，以及微信收发的文件。" },
  projects: { label: "项目与其他", color: "#14B8A6", hint: "用户目录下的其他文件夹，通常是代码项目。里面的构建产物可以在智能清理里处理。" },
  recycle: { label: "回收站", color: "#94A3B8", hint: "已删除但还在回收站里的文件。" },
  other: { label: "其他", color: "#CBD5E1", hint: "C 盘根目录下的其他文件夹。" },
  hidden: { label: "系统保留区", color: "#64748B", hint: "磁盘显示已用、但普通权限读不到的部分：系统还原点和卷影副本、NTFS 元数据，以及没有权限进入的文件夹。可以在「系统 › 系统保护」里调小还原点占用。" },
};

export const GROUPS: Record<Group, { label: string; hint: string }> = {
  system: { label: "系统垃圾", hint: "Windows 和程序产生的临时文件" },
  dev: { label: "开发缓存", hint: "可以重新下载或重新编译出来的东西" },
  app: { label: "应用缓存", hint: "浏览器等软件的缓存" },
  review: { label: "需要你确认", hint: "可以删，但请先看一眼" },
  guide: { label: "建议在应用内清理", hint: "直接删文件夹会出问题，这里只告诉你在哪清" },
};

export const GROUP_ORDER: Group[] = ["system", "dev", "app", "review", "guide"];

export const RISKS: Record<Risk, { label: string; tone: string }> = {
  safe: { label: "安全", tone: "safe" },
  caution: { label: "谨慎", tone: "caution" },
  review: { label: "需确认", tone: "review" },
};
