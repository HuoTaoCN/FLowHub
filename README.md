# FlowHub

FlowHub 是一个面向桌面的统一文件流转平台，目标是把局域网发现、点对点传输、下载管理、后续的同步与分享能力收敛到同一个应用里。

当前仓库处于 **v0.1 早期可运行阶段**：桌面壳、下载模块、局域网发现、文件传输底层和基础存储已经落地，整体架构已经搭好，但距离完整产品仍有一段距离。

## 项目定位

FlowHub 计划逐步提供以下能力：

- 局域网设备发现
- 设备间点对点文件发送
- 基于 aria2 的下载任务管理
- 多设备文件夹同步
- 带权限控制的文件分享

目前的实现重点是 **Send + Download MVP**，即先把“发现设备、发送文件、管理下载任务”这条主链路打通。

## 当前进度

基于当前代码，项目已经具备以下内容：

- 使用 **Rust workspace** 拆分核心能力模块：`core`、`discovery`、`send`、`download`、`storage`
- 使用 **Tauri v2** 作为桌面端后端壳层
- 使用 **React + Vite + TypeScript** 构建桌面前端界面
- 已完成基础界面：`Dashboard`、`Send`、`Download`、`Settings`
- 已实现 **UDP 局域网设备发现**
- 已实现 **TCP 文件发送/接收底层能力**
- 已实现 **aria2 JSON-RPC 下载接入**
- 已实现 **SQLite 任务与元数据存储**
- Rust 模块中已补充一定数量的单元测试

## 已实现功能

### 1. 桌面应用基础框架

- 应用可通过 Tauri 启动
- 前端已具备完整的侧边栏导航
- Dashboard 可同时展示设备、传输、下载和存储概览

### 2. 局域网发现

- 程序启动后会自动启动发现服务
- 通过 UDP 广播在局域网内发现其他设备
- 会维护一个内存中的设备列表，并自动过滤自身设备广播
- 前端可通过 Tauri 命令拉取设备列表

### 3. 文件发送与接收

- 已实现 TCP 文件传输协议雏形
- 发送端会附带文件名、总大小、续传起点和 SHA-256 校验信息
- 接收端会将文件保存到当前工作目录下的 `flowhub-received/`
- 已支持基础的传输进度回调
- 前端已可以选择设备、输入文件路径，并调用 Tauri 命令发起发送
- 传输完成后会在界面中显示状态

### 4. 下载管理

- 已接入 aria2 RPC
- 当前支持以下下载协议：
  - HTTP
  - HTTPS
  - FTP
  - Magnet
- 前端可新增、暂停、恢复、删除下载任务
- 界面会轮询后端状态并展示进度、速度和 ETA

### 5. 本地存储

- 使用 SQLite 保存任务元数据
- 已有下载任务状态持久化的基础能力
- 已实现简单的文件元数据表与任务表

## 当前未完成或仍需加强的部分

按照现在的实现状态，下面这些点仍属于“已起步但未完全产品化”：

- **发送体验仍偏底层**
  - 当前发送主要依赖“选择局域网设备 + 填写本地文件路径”
  - 还没有完整的文件选择器、发送任务管理和异常恢复流程
- **传输控制还不完整**
  - `send` crate 里有暂停/恢复控制能力
  - 但 UI 层尚未完整暴露暂停、恢复、重试等交互
- **设备发现仍是 MVP 级别**
  - 当前设备 ID 默认运行时生成，尚未做稳定持久化
  - 在线状态、TTL、离线体验还比较基础
- **下载模块依赖本地 aria2**
  - 需要用户自己启动 aria2 RPC 服务
  - 尚未内置 aria2 进程管理与自动检测
- **同步 / 分享模块还未开始**
  - 目前仓库的架构已为后续扩展预留位置
  - 但功能本身尚未落地

## 技术架构

```text
UI <-> Tauri Commands <-> flowhub-core <-> [discovery, send, download, storage]
```

仓库结构如下：

```text
.
├── app/
│   ├── src/                 # React 前端
│   └── src-tauri/           # Tauri 桌面端入口与命令桥接
├── crates/
│   ├── core/                # 应用编排层
│   ├── discovery/           # 局域网设备发现
│   ├── send/                # 文件发送与接收
│   ├── download/            # aria2 RPC 客户端
│   └── storage/             # SQLite 存储
├── Cargo.toml
├── package.json
└── README.md
```

## 模块说明

### `crates/core`

负责把发现、下载、存储等能力整合起来，对上提供统一应用接口。

### `crates/discovery`

负责局域网设备广播、接收、去重和设备列表维护。

### `crates/send`

负责 TCP 文件传输、基础续传、校验和传输进度计算。

### `crates/download`

负责与 aria2 JSON-RPC 通信，完成下载任务的增删改查。

### `crates/storage`

负责 SQLite 建表、文件元数据和任务元数据的持久化。

### `app/src-tauri`

负责 Tauri 应用入口、命令注册，以及把后端事件发给前端界面。

### `app/src`

负责桌面 UI，包括 Dashboard、发送页、下载页和设置页。

## 运行方式

### 环境依赖

需要提前准备：

- Rust toolchain
- Node.js / npm
- Tauri 对应平台依赖
- aria2

### 安装依赖

```bash
npm install
```

### 启动 aria2 RPC

```bash
aria2c --enable-rpc --rpc-listen-all=false --rpc-listen-port=6800
```

### 启动桌面应用

```bash
npm run tauri:dev
```

### 仅启动前端

```bash
npm run dev
```

### 运行 Rust 测试

```bash
cargo test --workspace
```

## 当前默认配置

- aria2 RPC 地址：`http://127.0.0.1:6800/jsonrpc`
- 局域网发现端口：`47321/UDP`
- 文件接收端口：`47322/TCP`
- SQLite 数据库：`flowhub.db`
- 接收文件目录：`flowhub-received/`

## 适合放在 GitHub 的项目简介

### 仓库摘要（较完整版本）

FlowHub 是一个基于 Tauri、Rust 和 React 构建的桌面文件流转平台，当前聚焦于局域网设备发现、点对点文件传输和 aria2 下载管理。项目采用 Rust workspace 拆分 discovery、send、download、storage 等模块，已经具备可运行的桌面壳、下载任务管理、局域网发现和基础文件传输能力，并为后续的文件同步与权限分享预留了架构空间。

### 仓库摘要（GitHub 简短版本）

基于 Tauri + Rust + React 的桌面文件流转平台，当前实现局域网设备发现、点对点文件传输和 aria2 下载管理，并为后续同步与分享能力预留架构。

## 下一步建议

如果继续按当前路线推进，比较适合优先做这几件事：

1. 完善发送流程：文件选择、任务列表、暂停/恢复/失败重试
2. 强化发现模块：稳定设备 ID、离线状态、TTL 策略
3. 提升下载体验：自动检测 aria2、任务恢复、错误处理
4. 打通同步与分享模块的第一版数据模型

## License

MIT
