# Apex Development Workflow

本文档描述了在 Apex 项目中进行功能开发、变更和缺陷修复的标准工作流程，以确保文档与代码的一致性。

## 1. 新增功能 (Feature)

### 步骤
1.  **更新 PRD**:
    - 在 [docs/PRD.md](PRD.md) 中添加或修改相关需求描述。
    - 确保新功能与现有架构一致。

2.  **创建/更新 Epic**:
    - 如果是大型功能模块，创建一个新的 Epic 文件（如 `docs/epics/E05-NewFeature.md`）。
    - 如果是现有模块的增强，在对应的 Epic 文件中添加新的 User Story。
    - **格式**:
        ```markdown
        - [ ] **S0x: Feature Name**
          - Description of the feature.
          - Acceptance criteria.
        ```

3.  **更新 Sprint Status**:
    - 编辑 [docs/sprint-status.yaml](sprint-status.yaml)。
    - 在对应的 Epic 下添加新的 Story 条目，并将状态标记为 `Pending`。
    - 示例:
        ```yaml
          stories:
            - id: S07
              name: New Feature Name
              status: Pending
        ```

4.  **代码开发**:
    - 编写代码实现功能。
    - 添加相应的测试用例。

5.  **更新状态**:
    - 功能开发完成并通过测试后，更新 [docs/sprint-status.yaml](sprint-status.yaml) 中的状态为 `Done`。
    - 在 Epic 文件中勾选对应的 Story。

## 2. 变更功能 (Change)

### 步骤
1.  **修订文档**:
    - 修改 [docs/PRD.md](PRD.md) 反映变更。
    - 更新对应 Epic 文件中的 Story 描述和验收标准。

2.  **重置状态 (可选)**:
    - 如果变更较大需要重新开发，将 [docs/sprint-status.yaml](sprint-status.yaml) 中对应 Story 的状态改为 `Pending` 或 `In Progress`。

3.  **代码修改**:
    - 修改代码以符合新的需求。

4.  **完成变更**:
    - 测试通过后，将状态更新回 `Done`。

## 3. 缺陷修复 (Bugfix)

### 步骤
1.  **记录缺陷**:
    - 简单的 Bug 可以直接修复。
    - 复杂的 Bug 建议在对应的 Epic 文件中添加一个 `Fix` 类型的 Story，或者创建一个专门的 `docs/epics/BUGS.md` 进行追踪。

2.  **添加任务**:
    - 在 [docs/sprint-status.yaml](sprint-status.yaml) 中添加 Bug 修复任务（可选，视 Bug 严重程度而定）。

3.  **修复与验证**:
    - 编写修复代码。
    - **必须**添加回归测试（Regression Test）以防止 Bug 再次出现。

4.  **关闭缺陷**:
    - 更新状态为 `Done`。

## 4. 常用命令

- **查看进度**:
  可以直接查看 `docs/sprint-status.yaml` 或使用脚本（如果已实现）生成报告。

- **文档结构**:
  - `docs/PRD.md`: 产品需求文档（单一事实来源）。
  - `docs/epics/*.md`: 详细的功能拆解和验收标准。
  - `docs/sprint-status.yaml`: 开发进度追踪表。
