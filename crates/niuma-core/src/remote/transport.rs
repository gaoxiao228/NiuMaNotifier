use super::e2ee::RpcFrame;

pub trait RemoteEncryptedTransport {
    /// 传输层只处理密文帧，不接触 RPC 明文。
    fn send_frame(&self, frame: RpcFrame);

    fn close(&self, reason: &str);
}
