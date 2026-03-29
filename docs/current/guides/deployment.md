# Apex Gateway - Deployment Guide

**Generated:** 2026-03-10
**Scope:** 部署流程和运维指南

---

## 部署方式

### 1. Docker 部署 (推荐)

#### 使用 Docker Compose

```yaml
# docker-compose.yml
version: '3.8'
services:
  apex:
    image: apex-gateway:latest
    container_name: apex-gateway
    ports:
      - "12356:12356"
    volumes:
      - ./config.json:/app/config.json:ro
      - apex-data:/root/.apex/data
      - ./logs:/root/.apex/logs
    restart: unless-stopped
    environment:
      - RUST_LOG=info

volumes:
  apex-data:
```

**启动服务**:
```bash
docker-compose up -d
```

**查看日志**:
```bash
docker-compose logs -f apex
```

**停止服务**:
```bash
docker-compose down
```

#### 使用 Docker 命令

```bash
# 构建镜像
docker build -t apex-gateway:latest .

# 运行容器
docker run -d \
  --name apex \
  -p 12356:12356 \
  -v $(pwd)/config.json:/app/config.json:ro \
  -v apex-data:/root/.apex/data \
  apex-gateway:latest
```

---

### 2. 手动部署

#### 系统要求

| 要求 | 说明 |
|------|------|
| OS | Linux (Ubuntu 20.04+), macOS |
| CPU | 2+ cores |
| Memory | 512MB+ |
| Disk | 1GB+ |

#### 安装步骤

**1. 下载二进制文件**:
```bash
# 直接安装最新版本
curl -fsSL https://raw.githubusercontent.com/cregis-dev/apex/main/install-release.sh | bash

# 或安装指定版本到 /opt/apex
curl -fsSL https://raw.githubusercontent.com/cregis-dev/apex/main/install-release.sh | \
  bash -s -- --version v0.1.0 /opt/apex

# 如需一并写入示例配置到明确路径
curl -fsSL https://raw.githubusercontent.com/cregis-dev/apex/main/install-release.sh | \
  bash -s -- --version v0.1.1 --config-path /opt/apex/config.json /opt/apex
```

**2. 移动到系统路径**:
```bash
sudo ln -sf /opt/apex/apex /usr/local/bin/apex
```

**3. 准备配置文件**:
```bash
sudo mkdir -p /opt/apex
curl -fsSL https://raw.githubusercontent.com/cregis-dev/apex/main/install-release.sh | \
  sudo bash -s -- --version v0.1.1 --config-path /opt/apex/config.json /opt/apex
sudo vi /opt/apex/config.json
```

**4. 配置 systemd 服务**:
```ini
# /etc/systemd/system/apex.service
[Unit]
Description=Apex AI Gateway
After=network.target

[Service]
Type=simple
User=apex
Group=apex
WorkingDirectory=/opt/apex
ExecStart=/usr/local/bin/apex gateway start --config /opt/apex/config.json
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

**5. 启动服务**:
```bash
# 创建用户
sudo useradd -r -s /bin/false apex

# 设置权限
sudo chown -R apex:apex /opt/apex

# 启用服务
sudo systemctl daemon-reload
sudo systemctl enable apex
sudo systemctl start apex
```

**6. 验证状态**:
```bash
sudo systemctl status apex
curl http://localhost:12356/metrics
```

---

### 3. 使用安装脚本

```bash
# 一键安装
curl -fsSL https://raw.githubusercontent.com/cregis-dev/apex/main/install-release.sh | sudo bash -s -- /opt/apex

# 安装并写入示例配置
curl -fsSL https://raw.githubusercontent.com/cregis-dev/apex/main/install-release.sh | \
  sudo bash -s -- --config-path /opt/apex/config.json /opt/apex
```

**安装脚本内容** (`install-release.sh`):
```bash
#!/bin/bash
INSTALL_PATH=${1:-/opt/apex}

echo "Installing Apex Gateway to $INSTALL_PATH"

# Detect platform and download the matching release archive
# Install only the apex binary by default
# Write config.example.json only when --config-path is provided

echo "Installation complete!"
echo "Run: $INSTALL_PATH/apex gateway start --config /path/to/config.json"
```

---

## 配置说明

### 生产环境配置示例

```json
{
  "version": "1.0",
  "global": {
    "listen": "0.0.0.0:12356",
    "auth": {
      "mode": "api_key",
      "keys": ["sk-prod-global-key"]
    },
    "timeouts": {
      "connect_ms": 2000,
      "request_ms": 30000,
      "response_ms": 60000
    },
    "retries": {
      "max_attempts": 3,
      "backoff_ms": 500,
      "retry_on_status": [429, 500, 502, 503, 504]
    },
    "cors_allowed_origins": ["https://dashboard.example.com"]
  },
  "logging": {
    "level": "info",
    "dir": "/var/log/apex"
  },
  "data_dir": "/var/lib/apex/data",
  "channels": [
    {
      "name": "openai-prod",
      "provider": "openai",
      "base_url": "https://api.openai.com",
      "api_key": "sk-prod-key"
    },
    {
      "name": "openai-backup",
      "provider": "openai",
      "base_url": "https://api.openai.com",
      "api_key": "sk-backup-key"
    }
  ],
  "routers": [
    {
      "name": "main-router",
      "rules": [
        {
          "match": { "model": "gpt-4*" },
          "strategy": "priority",
          "channels": [
            { "name": "openai-prod", "weight": 1 },
            { "name": "openai-backup", "weight": 1 }
          ]
        }
      ]
    }
  ],
  "teams": [
    {
      "id": "prod-team",
      "api_key": "sk-ap-prod-key",
      "policy": {
        "allowed_routers": ["main-router"],
        "rate_limit": {
          "rpm": 1000,
          "tpm": 500000
        }
      }
    }
  ],
  "metrics": {
    "enabled": true,
    "path": "/metrics"
  },
  "hot_reload": {
    "config_path": "/opt/apex/config.json",
    "watch": true
  }
}
```

说明：

- 发布二进制应使用 `embedded-web` feature 构建
- 发布产物不再需要额外携带 `web/` 静态目录
- `web_dir` 仅在开发态或未启用 `embedded-web` 的文件系统模式下使用

---

## 监控和告警

### Prometheus 配置

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'apex'
    static_configs:
      - targets: ['localhost:12356']
    metrics_path: '/metrics'
    scrape_interval: 15s
```

### Grafana Dashboard

导入以下 Panel:

| Panel | Query |
|-------|-------|
| 请求总量 | `sum(apex_requests_total)` |
| 错误率 | `sum(apex_errors_total) / sum(apex_requests_total) * 100` |
| P95 延迟 | `histogram_quantile(0.95, apex_upstream_latency_ms_bucket)` |
| Fallback 次数 | `sum(apex_fallbacks_total)` |

### 告警规则

```yaml
# alerting.yml
groups:
  - name: apex
    rules:
      - alert: HighErrorRate
        expr: sum(rate(apex_errors_total[5m])) / sum(rate(apex_requests_total[5m])) > 0.05
        for: 5m
        annotations:
          summary: "Apex 错误率超过 5%"

      - alert: HighLatency
        expr: histogram_quantile(0.95, apex_upstream_latency_ms_bucket) > 5000
        for: 5m
        annotations:
          summary: "Apex P95 延迟超过 5 秒"
```

---

## 备份和恢复

### 备份数据库

```bash
# 备份 SQLite 数据库
cp /var/lib/apex/data/apex.db /var/lib/apex/data/apex.db.backup.$(date +%Y%m%d)

# 压缩备份
tar -czf /backup/apex-data-$(date +%Y%m%d).tar.gz /var/lib/apex/data/
```

### 恢复数据库

```bash
# 停止服务
sudo systemctl stop apex

# 恢复数据库
cp /var/lib/apex/data/apex.db.backup.20240101 /var/lib/apex/data/apex.db

# 启动服务
sudo systemctl start apex
```

---

## 升级流程

### 1. 备份当前配置

```bash
cp -r /opt/apex /opt/apex.backup.$(date +%Y%m%d)
```

### 2. 下载新版本

```bash
wget https://github.com/cregis-dev/apex/releases/download/v0.2.0/apex-x86_64-unknown-linux-musl.tar.gz
tar -xzf apex-*.tar.gz
```

### 3. 替换二进制文件

```bash
sudo systemctl stop apex
sudo mv apex /usr/local/bin/apex
sudo systemctl start apex
```

### 4. 验证升级

```bash
apex --version
curl http://localhost:12356/metrics
sudo systemctl status apex
```

---

## 故障排查

### 常见问题

**Q: 服务无法启动？**

A: 检查日志：
```bash
sudo journalctl -u apex -f
```

**Q: API 请求超时？**

A: 检查上游 Provider 状态和网络连接：
```bash
curl -I https://api.openai.com
```

**Q: Dashboard 无法访问？**

A: 发布环境优先确认是否使用 `embedded-web` 构建；仅文件系统模式下才需要检查 `web_dir` 和 `target/web`：
```bash
cargo build --release --features embedded-web
```

---

_Generated using BMAD Method `document-project` workflow_
