//! 连接池客户端的单元测试

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::{Config, ConnectionMode}, models::{Pos, Rgb}};

    /// 测试 PoolClient 创建
    #[tokio::test]
    async fn test_pool_client_creation() {
        let config = Config::default();
        let client = PoolClient::new(config).await;

        assert!(client.is_ok());
    }

    /// 测试健康状态管理
    #[tokio::test]
    async fn test_health_status_management() {
        let config = Config::default();
        let mut client = crate::basic_client::BasicClient::new_impl(config).await.unwrap();

        // 初始状态应该是健康的
        assert!(client.is_healthy());

        // 标记为不健康
        client.mark_unhealthy();
        assert!(!client.is_healthy());

        // 重置健康状态
        client.reset_health();
        assert!(client.is_healthy());
    }

    /// 测试 WriteOnlyManager 创建连接
    #[tokio::test]
    async fn test_write_only_manager_create() {
        let config = Arc::new(Config::default());
        let manager = WriteOnlyManager::new(config);

        let client = manager.create().await;
        assert!(client.is_ok());

        let mut client = client.unwrap();
        assert!(client.is_healthy());
    }

    /// 测试 WriteOnlyManager 回收逻辑
    #[tokio::test]
    async fn test_write_only_manager_recycle() {
        let config = Arc::new(Config::default());
        let manager = WriteOnlyManager::new(config);

        let mut client = crate::basic_client::BasicClient::new_impl(Config::default()).await.unwrap();

        // 健康连接应该被回收
        assert!(client.is_healthy());
        let metrics = deadpool::managed::Metrics::default();
        let result = manager.recycle(&mut client, &metrics).await;
        assert!(result.is_ok());

        // 不健康连接应该被拒绝回收
        client.mark_unhealthy();
        let result = manager.recycle(&mut client, &metrics).await;
        assert!(result.is_err());
    }

    /// 测试 WriteOnlyPool 创建和获取连接
    #[tokio::test]
    async fn test_write_only_pool() {
        let config = Arc::new(Config::default());
        let pool = WriteOnlyPool::new(config).await.unwrap();

        // 获取连接应该成功
        let conn = pool.get().await;
        assert!(conn.is_ok());
    }

    /// 测试工厂模式支持 Pool 类型
    #[tokio::test]
    async fn test_factory_pool_type() {
        let config = Config::default();

        // 测试创建 Pool 类型的客户端
        let client = crate::basic_client::create_client_by_type(config, crate::basic_client::ClientType::Pool).await;

        assert!(client.is_ok());
        let client_box = client.unwrap();
        assert!(client_box.get_config().connection_mode != ConnectionMode::ReadWrite);
    }
}