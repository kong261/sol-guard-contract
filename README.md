# SolGuard 🛡

**Solana 上的去中心化资产守护协议**

> "你的资产，你来守护——用代码，不用运气。"

---

## 什么是 SolGuard？

SolGuard 是 Solana 上首个去中心化资产守护协议。它保护你的链上资产免受四种最常见的威胁：

- 🔓 **私钥被盗** — 时间锁给你留出取消提款的时间，Guardian 一键取消
- 🔫 **绑架胁迫** — 胁迫钱包悄悄把时间锁延长至30天，绑匪毫不知情
- 💀 **意外离世/失能** — 心跳机制 + 多签 Guardian 继承资产
- 🔑 **密钥丢失** — Guardian 可以帮你更换 Owner 地址到新钱包

传统钱包的安全性100%依赖一把私钥——私钥丢了，钱就没了。SolGuard 把"一把钥匙决定一切"变成了"规则决定一切"，让你的资产在任何极端情况下都能按你的意愿执行。

---

## 核心机制

| 功能 | 说明 |
|------|------|
| ⏱ 时间锁 | 所有提款都有强制等待期，金额越大等待越长 |
| 🔑 胁迫钱包 | 备用钱包连接时自动触发30天时间锁，界面与正常完全一样 |
| 👥 多签 Guardian | 信任的人可以冻结金库、取消提款、更换 Owner 地址 |
| 💓 心跳签到 | 每月签到证明活跃，超过90天未签到则 Guardian 可继承资产 |

---

## 时间锁规则

| 金额 | 等待时间 |
|------|---------|
| 不足 1 SOL | 1 小时 |
| 1–10 SOL | 6 小时 |
| 10–100 SOL | 3 天 |
| 超过 100 SOL | 14 天 |
| 胁迫钱包签名 | 30 天 |
| 全额一次性转出 | 禁止 |

---

## 合约指令

```
initialize_vault          — 创建金库，设置 Guardian 和胁迫钱包
deposit                   — 存入 SOL
heartbeat                 — 每月心跳签到，重置失联计时器
initiate_withdrawal       — 发起提款申请（时间锁开始计时）
execute_withdrawal        — 时间锁到期后执行提款（任何人可触发）
cancel_withdrawal         — 取消待处理提款（Owner 或任意 Guardian）
emergency_freeze          — 紧急冻结金库（Owner 或任意 Guardian，立即生效）
owner_unfreeze            — 解冻金库（仅 Owner）
create_guardian_proposal  — 发起密钥轮换或继承提案
approve_guardian_proposal — Guardian 签名批准提案
execute_guardian_proposal — 达到阈值后执行提案
```

---

## Guardian 权限设计

| 操作 | 所需签名 | 说明 |
|------|---------|------|
| 紧急冻结 | 1/N | 任意一个 Guardian 立即生效 |
| 取消提款 | 1/N | 任意一个 Guardian 立即生效 |
| 更换 Owner 地址 | 2/N（可配置） | 防止单个 Guardian 叛变 |
| 继承资产 | 2/N（可配置） | Owner 失联90天后方可发起 |
| 发起提款 | 永远不可以 | Guardian 无法主动转走资产 |

---

## 部署信息

- **网络：** Solana Devnet
- **程序ID：** `6LfqYJ1UgRsu97kRUwTC8W8Mzq5nFCnQcTmm69kdaBfn`
- **前端网站：** https://sol-guard-frontend.vercel.app
- **前端仓库：** https://github.com/kong261/sol-guard-frontend

---

## 技术栈

- **智能合约：** Rust + Anchor 0.30.1
- **前端：** Next.js + Tailwind CSS + @solana/wallet-adapter + @coral-xyz/anchor
- **网络：** Solana Devnet
- **部署平台：** Solana Playground（合约）/ Vercel（前端）

---

## 本地运行

```bash
# 克隆仓库
git clone https://github.com/kong261/sol-guard-contract
cd sol-guard-contract

# 编译合约
anchor build

# 运行测试
anchor test --provider.cluster devnet

# 部署到 Devnet
anchor deploy
```

---

## 参赛信息

Solana Frontier Hackathon 2026 — [arena.colosseum.org](https://arena.colosseum.org)

---

*SolGuard — 链上信托，用智能合约替代律师，用代码替代法院。*
*有人的地方就有中心化，SolGuard 让你的资产在任何极端情况下都不受中心化强权的掌控。*
