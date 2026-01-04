use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

// 官方公钥 (Base64 编码)
// 优先读取环境变量 NBOT_OFFICIAL_PUBLIC_KEY_B64；未设置则使用默认值。
// 默认值是示例公钥：生产部署必须替换为实际公钥。
const DEFAULT_OFFICIAL_PUBLIC_KEY_B64: &str =
    "MCowBQYDK2VwAyEAKxpJCvLJGxGqPvKam8M8xqJ5VnGqFqHHxGqKxqKxqKo=";

fn official_public_key_b64() -> String {
    std::env::var("NBOT_OFFICIAL_PUBLIC_KEY_B64")
        .unwrap_or_else(|_| DEFAULT_OFFICIAL_PUBLIC_KEY_B64.to_string())
}

pub struct PluginVerifier {
    public_key: VerifyingKey,
}

fn hash_payload_files(files: &[(&str, &[u8])]) -> [u8; 32] {
    // Deterministic payload hash for directory plugins.
    // We intentionally exclude manifest.json (config is user-writable) and hash only package files.
    //
    // Hash input = concat over sorted paths:
    //   path_bytes + 0x00 + file_bytes + 0x00
    let mut items: Vec<(&str, &[u8])> = files.to_vec();
    items.sort_by(|a, b| a.0.cmp(b.0));

    let mut hasher = Sha256::new();
    for (path, data) in items {
        hasher.update(path.as_bytes());
        hasher.update([0u8]);
        hasher.update(data);
        hasher.update([0u8]);
    }
    hasher.finalize().into()
}

impl PluginVerifier {
    pub fn new() -> Result<Self, String> {
        // 解码公钥
        let key_b64 = official_public_key_b64();
        let key_bytes = BASE64
            .decode(&key_b64)
            .map_err(|e| format!("Failed to decode public key: {}", e))?;

        // Ed25519 公钥是 32 字节
        let key_array: [u8; 32] = if key_bytes.len() == 32 {
            key_bytes
                .try_into()
                .map_err(|_| "Failed to convert public key".to_string())?
        } else if key_bytes.len() > 32 {
            // 兼容部分以 DER/SPKI 形式 base64 的输入：取末尾 32 字节作为公钥材料
            let slice = &key_bytes[key_bytes.len() - 32..];
            slice
                .try_into()
                .map_err(|_| "Failed to convert public key".to_string())?
        } else {
            return Err("Invalid public key length".to_string());
        };

        let public_key = VerifyingKey::from_bytes(&key_array)
            .map_err(|e| format!("Invalid public key: {}", e))?;

        Ok(Self { public_key })
    }

    /// 验证插件签名
    /// 签名内容 = SHA256(plugin_code) + plugin_id + version
    pub fn verify(
        &self,
        plugin_id: &str,
        version: &str,
        code: &[u8],
        signature_b64: &str,
    ) -> Result<bool, String> {
        // 解码签名
        let signature_bytes = BASE64
            .decode(signature_b64)
            .map_err(|e| format!("Failed to decode signature: {}", e))?;

        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|e| format!("Invalid signature format: {}", e))?;

        // 构建签名内容
        let mut hasher = Sha256::new();
        hasher.update(code);
        let code_hash = hasher.finalize();

        let mut message = Vec::new();
        message.extend_from_slice(&code_hash);
        message.extend_from_slice(plugin_id.as_bytes());
        message.extend_from_slice(version.as_bytes());

        match self.public_key.verify(&message, &signature) {
            Ok(_) => {
                info!("插件 {} v{} 签名验证通过", plugin_id, version);
                Ok(true)
            }
            Err(_) => {
                warn!("插件 {} v{} 签名验证失败", plugin_id, version);
                Ok(false)
            }
        }
    }

    /// 验证目录/多文件插件签名（推荐）
    /// 签名内容 = SHA256(payload_files) + plugin_id + version
    /// - payload_files: 包内文件（不含 manifest.json），按相对路径排序后参与哈希。
    pub fn verify_payload(
        &self,
        plugin_id: &str,
        version: &str,
        files: &[(&str, &[u8])],
        signature_b64: &str,
    ) -> Result<bool, String> {
        let signature_bytes = BASE64
            .decode(signature_b64)
            .map_err(|e| format!("Failed to decode signature: {}", e))?;

        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|e| format!("Invalid signature format: {}", e))?;

        let payload_hash = hash_payload_files(files);

        let mut message = Vec::new();
        message.extend_from_slice(&payload_hash);
        message.extend_from_slice(plugin_id.as_bytes());
        message.extend_from_slice(version.as_bytes());

        match self.public_key.verify(&message, &signature) {
            Ok(_) => {
                info!("插件 {} v{} payload 签名验证通过", plugin_id, version);
                Ok(true)
            }
            Err(_) => {
                warn!("插件 {} v{} payload 签名验证失败", plugin_id, version);
                Ok(false)
            }
        }
    }
}

/// 用于签名服务：签名插件
pub fn sign_plugin(
    private_key_b64: &str,
    plugin_id: &str,
    version: &str,
    code: &[u8],
) -> Result<String, String> {
    let key_bytes = BASE64
        .decode(private_key_b64)
        .map_err(|e| format!("Failed to decode private key: {}", e))?;

    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| "Invalid private key length")?;

    let signing_key = SigningKey::from_bytes(&key_array);

    // 构建签名内容
    let mut hasher = Sha256::new();
    hasher.update(code);
    let code_hash = hasher.finalize();

    let mut message = Vec::new();
    message.extend_from_slice(&code_hash);
    message.extend_from_slice(plugin_id.as_bytes());
    message.extend_from_slice(version.as_bytes());

    let signature = signing_key.sign(&message);
    Ok(BASE64.encode(signature.to_bytes()))
}

/// 用于签名服务：签名目录/多文件插件（推荐）
#[allow(dead_code)]
pub fn sign_plugin_payload(
    private_key_b64: &str,
    plugin_id: &str,
    version: &str,
    files: &[(&str, &[u8])],
) -> Result<String, String> {
    let key_bytes = BASE64
        .decode(private_key_b64)
        .map_err(|e| format!("Failed to decode private key: {}", e))?;

    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| "Invalid private key length")?;

    let signing_key = SigningKey::from_bytes(&key_array);

    let payload_hash = hash_payload_files(files);

    let mut message = Vec::new();
    message.extend_from_slice(&payload_hash);
    message.extend_from_slice(plugin_id.as_bytes());
    message.extend_from_slice(version.as_bytes());

    let signature = signing_key.sign(&message);
    Ok(BASE64.encode(signature.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify() -> Result<(), String> {
        use rand_core::OsRng;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let private_key = BASE64.encode(signing_key.to_bytes());
        let public_key = BASE64.encode(verifying_key.to_bytes());

        let plugin_id = "test-plugin";
        let version = "1.0.0";
        let code = b"console.log('hello');";

        let signature = sign_plugin(&private_key, plugin_id, version, code)?;

        // 使用生成的公钥验证
        let key_bytes = BASE64
            .decode(&public_key)
            .map_err(|e| format!("decode public key failed: {e}"))?;
        let key_array: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| "Invalid public key length".to_string())?;
        let verifying_key =
            VerifyingKey::from_bytes(&key_array).map_err(|e| format!("Invalid public key: {e}"))?;

        let verifier = PluginVerifier {
            public_key: verifying_key,
        };
        let ok = verifier.verify(plugin_id, version, code, &signature)?;
        assert!(ok);
        Ok(())
    }
}
