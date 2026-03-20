# Web 目录规范化与迁移清单

**Project:** Apex Gateway  
**Date:** 2026-03-11  
**Scope:** `web/` 目录结构梳理，不修改业务逻辑

---

## 1. 目标

在推进 `embedded-web` 方案之前，先把前端源码目录、构建缓存目录、静态导出目录、测试产物目录分层。

本次文档只定义规范化方案和迁移清单，不执行删除、移动或构建逻辑改造。

---

## 2. 当前状态结论

当前 `web/` 目录同时承载了 5 类内容：

1. 前端源码与配置
2. Next.js 本地构建缓存
3. Next.js 静态导出产物
4. 本地依赖与测试输出
5. 一个嵌套的 Git 仓库

这使得 `web/` 目录职责不清晰，也会直接影响后续 `embedded-web` 的嵌入边界。

---

## 3. 当前目录分类

### 3.1 应保留在 `web/` 的源码与配置

- `web/src/`
- `web/public/`
- `web/tests/`
- `web/package.json`
- `web/package-lock.json`
- `web/next.config.ts`
- `web/tsconfig.json`
- `web/postcss.config.mjs`
- `web/eslint.config.mjs`
- `web/playwright.config.ts`
- `web/components.json`
- `web/test-config.json`
- `web/src/app/favicon.ico`

### 3.2 本地开发缓存或依赖，不应作为仓库结构基线

- `web/.next/`
- `web/node_modules/`
- `web/test-results/`
- `web/tsconfig.tsbuildinfo`
- `web/.env.local`

说明：

- 这些内容允许本地存在
- 但不应参与发布基线
- 也不应成为 embedded 资源来源

### 3.3 静态导出产物，应该离开 `web/` 源码根目录

以下内容本质上属于 `next export` 结果或其副产物：

- `web/_next/`
- `web/_not-found/`
- `web/dashboard/`
- `web/404/`
- `web/index.html`
- `web/404.html`
- `web/favicon.ico`
- `web/*.svg`
- `web/index.txt`
- `web/__next.*.txt`
- `web/out/`

说明：

- `_next`、`_not-found` 这类以下划线开头的目录本身不是异常
- 问题在于这些导出产物出现在源码目录根层
- 当前规范应是导出到 `target/web/`，而不是留在 `web/`

### 3.4 结构性风险项

- `web/.git/`

说明：

- `web/` 当前是一个嵌套 Git 仓库
- 顶层仓库对 `web/` 的追踪状态异常，`git status` 显示为 `?? web/`
- 在此状态下推进结构治理和后续发布方案，风险过高

---

## 4. 目标结构

建议收敛为以下结构：

```text
apex/
├─ src/                      # Rust backend
├─ target/
│  └─ web/                   # Next.js export 产物，仅构建输出
├─ web/
│  ├─ src/                   # Next.js source
│  ├─ public/                # Static source assets
│  ├─ tests/                 # Playwright tests
│  ├─ package.json
│  ├─ package-lock.json
│  ├─ next.config.ts
│  ├─ tsconfig.json
│  ├─ postcss.config.mjs
│  ├─ eslint.config.mjs
│  ├─ playwright.config.ts
│  └─ README.md
└─ docs/
```

约束如下：

- `web/` 只放源码和前端配置
- `target/web/` 只放导出产物
- Rust 服务端只从 `target/web/` 或 embedded 资产读取静态内容
- 不允许把 `out/` 内容复制回 `web/` 根目录

---

## 5. 推荐迁移顺序

### Phase 1. 先收口版本控制边界

目标：

- 确认 `web/.git/` 是否为历史遗留子仓库
- 统一由顶层仓库管理 `web/`

动作：

- 审核 `web/.git/` 是否还需要保留
- 若不需要，迁出或移除嵌套仓库元数据
- 在顶层仓库重新纳入 `web/` 源码文件

验收标准：

- 顶层 `git status` 不再把整个 `web/` 视作未追踪黑盒

### Phase 2. 清理源码目录中的导出产物

目标：

- `web/` 根目录只保留源码与配置

拟清理对象：

- `web/_next/`
- `web/_not-found/`
- `web/dashboard/`
- `web/404/`
- `web/index.html`
- `web/404.html`
- `web/index.txt`
- `web/__next.*.txt`
- `web/out/`
- 根目录下由导出复制出来的静态资源文件

验收标准：

- `web/` 根目录不再出现 HTML 导出页、`_next` 产物树、`__next.*.txt`

### Phase 3. 校准构建与发布约定

目标：

- 保证唯一合法构建输出目录是 `target/web/`

动作：

- 保持 `web/package.json` 的构建脚本输出到 `target/web/`
- 检查是否存在其他脚本把导出结果回写到 `web/`
- 更新文档，避免开发者误将 `out/` 或导出产物提交到源码目录

验收标准：

- 运行构建后，产物只出现在 `target/web/`

### Phase 4. 再推进 `embedded-web`

目标：

- 以 `target/web/` 作为嵌入输入目录

动作：

- 引入 `embedded-web` feature
- 统一静态文件访问层
- 保留开发态文件系统读取模式

验收标准：

- 发布态只需要分发 Rust 二进制

---

## 6. 清单：保留 / 忽略 / 清理

### 6.1 保留并纳入顶层仓库

- `web/src/**`
- `web/public/**`
- `web/tests/**`
- `web/package.json`
- `web/package-lock.json`
- `web/next.config.ts`
- `web/tsconfig.json`
- `web/postcss.config.mjs`
- `web/eslint.config.mjs`
- `web/playwright.config.ts`
- `web/components.json`
- `web/test-config.json`
- `web/README.md`，但内容需要改写

### 6.2 本地忽略即可

- `web/.next/**`
- `web/node_modules/**`
- `web/test-results/**`
- `web/.env.local`
- `web/tsconfig.tsbuildinfo`

### 6.3 应从源码目录移除

- `web/_next/**`
- `web/_not-found/**`
- `web/dashboard/**`
- `web/404/**`
- `web/out/**`
- `web/index.html`
- `web/404.html`
- `web/index.txt`
- `web/__next.__PAGE__.txt`
- `web/__next._full.txt`
- `web/__next._head.txt`
- `web/__next._index.txt`
- `web/__next._tree.txt`
- `web/dashboard/*.txt`
- `web/_not-found/*.txt`

### 6.4 需单独决策

- `web/.git/`

这是治理前必须确认的阻塞项。

---

## 7. 文档和流程同步建议

以下文档后续应同步修正：

- `web/README.md`
- `README.md`
- `docs/current/guides/development-web.md`
- `docs/current/guides/deployment.md`
- 任何提到“产物位于 `web/` 根目录”的说明

需要明确写清：

- 源码目录是 `web/`
- 构建输出目录是 `target/web/`
- 发布态不依赖 `web/` 目录

---

## 8. 建议的下一步

推荐按下面顺序继续：

1. 先处理 `web/.git/` 的归属问题
2. 清掉 `web/` 根目录中的静态导出产物
3. 改写 `web/README.md` 和相关开发文档
4. 再开始 `embedded-web` 实现

如果直接在当前混合结构上做 embedded，容易把错误目录一起打进二进制，后续维护成本会持续放大。
