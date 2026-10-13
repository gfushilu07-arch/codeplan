# gitcode.com 页面 brief(实测证据)

> 用途:CodePlan 前端视觉基线。方法参考 `ai-website-cloner-template` 的 Map→Observe→Build→Compare,
> 但**只取布局系统与视觉语言**,不复制 gitcode 的品牌标识、文案与素材;CodePlan 保留自有品牌。
> 证据来源:浏览器 `getComputedStyle` 实测(非截图目测),采样于 2026-10-11,视口 1280×720。

## 1. 设计令牌(源码声明的 DevUI 变量,直接读取)

配色**全量照搬**,不做近似替代。CodePlan 令牌 → gitcode DevUI 变量:

| CodePlan | gitcode DevUI | 值 |
|---|---|---|
| `--cp-color-primary` | `--devui-brand` | `#165dff` |
| `--cp-color-primary-hover` | `--devui-brand-active` | `#526ecc` |
| `--cp-color-primary-active` | `--devui-brand-active-focus` | `#344899` |
| `--cp-color-primary-soft` | `--devui-primary-bg` | `#f2f5fc` |
| `--cp-color-link` | `--devui-link` | `#2951e0` |
| `--cp-color-solid` | `--devui-primary` | `#000000` |
| `--cp-color-solid-hover` | `--devui-primary-hover` | `#333333` |
| `--cp-color-text` | `--devui-text` | `#252b3a` |
| `--cp-color-text-secondary` | `--devui-text-weak` | `#3b3e55` |
| `--cp-color-text-muted` | `--devui-placeholder` | `#9c9db4` |
| `--cp-color-text-disabled` | `--devui-disabled-text` | `#bcbcd0` |
| `--cp-color-surface` | `--devui-global-bg` | `#f9f9fb` |
| `--cp-color-surface-alt` | `--devui-area` | `#f1f1f8` |
| `--cp-color-surface-hover` | `--devui-icon-hover-bg` | `#f1f1f8` |
| `--cp-color-border` | `--devui-line` | `#e3e3ee` |
| `--cp-color-border-light` | `--devui-dividing-line` | `#f1f1f8` |
| `--cp-color-success` / `-fg` / `-soft` | `--devui-success` / `-text` / `-bg` | `#088f4e` / `#31a26c` / `#d6ece2` |
| `--cp-color-warning` / `-fg` / `-soft` | `--devui-warning` / `-text` / `-bg` | `#ed5f22` / `#f07a47` / `#fce4da` |
| `--cp-color-danger` / `-soft` | `--devui-danger` / `-bg` | `#da203e` / `#f9dadf` |
| `--cp-color-info` / `-soft` | `--devui-info` / `-bg` | `#5e7ce0` / `#e9edfa` |

其余非变量实测字面值:卡片描述/页脚 `#606060`,卡片与操作按钮边框 `#f3f4f6`,排名徽章底 `#ffe0d4` 字 `#b1472d`,筛选胶囊激活底 `#0f0f0f` 字 `#f1f1f1`,卡片阴影 `rgba(0,0,0,.03) 0 4px 4px`。

结构令牌:`--devui-border-radius: 4px`、顶栏 56px、内容 1210px、正文 14px / 次级 12px。

> **已知取舍**:`#9c9db4` 对白底对比度仅 **2.53:1**,低于 WCAG AA(gitcode 自身如此)。为 1:1 克隆配色予以保留;若要无障碍达标,把 `--cp-color-text-muted` 改为 `#6b7280`(≈4.5:1)即可,一行生效。

## 2. 排版比例(按出现频次统计的 `size/weight/line-height`)

| 规格 | 用例 |
|---|---|
| 14 / 400 / 22 | 正文(基准) |
| 14 / 600 / 22 | 卡片标题、激活标签 |
| 16 / 700 / 25 | 大卡片标题 |
| 13 / 600 / 22 | 次级按钮文字 |
| 12 / 400 / 20 | 页脚、元信息 |
| 12 / 500 / 16 | 分区标签 |
| 11 / 400 / 16 | 标签(tag) |

行高统一约 `1.55–1.57`。

## 3. 页面骨架

**顶栏**:高 `56px`,左 = logo + 主导航,右 = 搜索 + 用户区;实心"登录"按钮为黑底白字,圆角 4px,高 32px。

**仓库页**(对 CodePlan 最重要):

```
内容容器          1210px
repo-header       53px   owner/repo 标题 + 标签栏
标签栏            48px   项间距 32px,激活项 weight 600 + 底部下划线
分支工具栏        32px   分支下拉 + 提交数/分支数 | 右侧 ZIP / Clone + 文件搜索框(190×32)
主体两栏:
  main            778px  文件列表 / 提交列表
  aside           400px  Introduction(README)面板
```

两栏比例约 `66 / 34`。

**首页(发现页)**:

```
左频道栏          240px   底 #f9f9fb,右 1px #e3e3ee
  频道项          36px    圆角 6px,padding 7px 10px,14px
  激活项          底 #f1f1f7,weight 600,字色 #252b3a
  分组标题        12px/500 #6b7280,padding 12px 12px 4px
主区
  横幅 hero      110px    居中胶囊公告
  筛选胶囊行      53px    底边框 #f1f1f8
  区块标题        18px/600 #252b3a
  卡片网格        2 列,间距 16px(横纵一致)
```

**卡片**(实测 485×151,间距 16px):

| 部位 | 实测值 |
|---|---|
| 容器 | 圆角 8px,padding 12px,边框 1px,阴影 `rgba(0,0,0,.03) 0 4px 4px` |
| 排名徽章 | 高 26px,11px/700,底 `#ffe0d4`,字 `#b1472d`,圆角 4px,padding `0 7px` |
| 名称 | 15px/600,`#000` |
| 操作按钮 | 高 24px,13px/600,圆角 4px,边框 1px,padding `0 12px` |
| 描述 | 14px / 行高 22px,`#606060`,**-webkit-line-clamp: 2** |
| 页脚 | flex,gap 8px,14px,`#606060`(语言点 + 标签 + 时间) |

**筛选胶囊**:高 32px,圆角 8px,14px/600;激活态底 `#0f0f0f`、字 `#f1f1f1`;非激活底 `#f2f2f2`、字 `#0f0f0f`。

## 4. 组件几何

| 组件 | 实测值 |
|---|---|
| 卡片 | 圆角 8px,padding 12px,内部 gap 16px |
| 实心按钮(Clone/Login) | 底色 `#000`,文字白,圆角 4px,高 32px,padding `0 9px` |
| 描边按钮(Star/Fork) | 白底 + 1px `#e3e3ee`,高 32px |
| 标签页 | 高 48px,间距 32px,激活 600 字重 + 下划线 |
| 侧栏导航项(激活) | 底 `#f1f1f7`,圆角 6px,高 36px |
| 文件搜索框 | 190×32 |
| 侧栏面板 | 宽 240px,底 `#f9f9fb` |

## 5. 与 CodePlan 的映射

| gitcode | CodePlan |
|---|---|
| `--devui-brand #165dff` | `--cp-color-primary` |
| `--devui-primary #000` 实心按钮 | `.cp-btn-primary`(实心黑) |
| `--devui-text #252b3a` | `--cp-color-text` |
| `--devui-line #e3e3ee` | `--cp-color-border` |
| `#f9f9fb` 面板底 | `--cp-color-surface` |
| 仓库页 main+aside 双栏 | `RepoPage` 重构 |

## 6. 有意偏离(不照抄)

- **配色不再偏离**:已按用户要求全量照搬 DevUI 原值(含 `#9c9db4`,对比度取舍见 §1 注)。
- 不复制 gitcode 的 logo、图标素材、文案与品牌名;`CP` 标识与渐变沿用 CodePlan 自有。
- 保留 CodePlan 自有信息架构(工作台/需求池/甘特/报表等 gitcode 没有的页面),仅套用其布局系统与视觉语言。
- 卡片上的排名/Star 等 gitcode 专有数据不伪造:排名用真实顺序,操作按钮改为真实可用的「打开」,语言点位置放项目 key。
