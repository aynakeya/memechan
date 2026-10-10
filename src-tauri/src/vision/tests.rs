use super::*;
use crate::test_support::{completion, server};

#[test]
fn vision_sends_animation_frames_and_rejects_failed_or_incomplete_descriptions() -> Result<()> {
    let secret = "test-only-credential";
    let responses = vec![
        (200, completion("角色先敬礼，再趴下打滚")),
        (
            401,
            json!({"error":{"message":format!("Invalid key {secret}")}}),
        ),
        (
            200,
            json!({"choices":[{"finish_reason":"length","message":{"content":"截断的描述"}}]}),
        ),
        (
            200,
            json!({"choices":[{"finish_reason":"stop","message":{"content":null,"reasoning_content":"分析"}}]}),
        ),
    ];
    let (base_url, server) = server(responses, |_| {})?;
    let client = VisionClient::new(VisionConfig {
        base_url,
        api_key: secret.into(),
        ..Default::default()
    })?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("action.gif");
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(std::fs::File::create(&path)?);
        for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            encoder.encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                16,
                8,
                image::Rgba(color),
            )))?;
        }
    }
    assert_eq!(client.describe(&path)?.text, "角色先敬礼，再趴下打滚");
    let error = client.probe().err().unwrap().to_string();
    assert!(error.contains("401") && !error.contains(secret));
    assert!(client.probe().err().unwrap().to_string().contains("截断"));
    assert!(client
        .probe()
        .err()
        .unwrap()
        .to_string()
        .contains("有效描述"));
    let requests = server.join().unwrap();
    let parts = requests[0]["messages"][0]["content"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    for part in &parts[1..] {
        let encoded = part["image_url"]["url"]
            .as_str()
            .unwrap()
            .strip_prefix("data:image/jpeg;base64,")
            .unwrap();
        let bytes = STANDARD.decode(encoded)?;
        assert_eq!(image::load_from_memory(&bytes)?.width(), 16);
    }
    assert_ne!(parts[1], parts[2]);
    assert!(requests
        .iter()
        .all(|request| !request.to_string().contains(secret)));
    Ok(())
}

#[test]
fn api_configuration_validates_addresses_and_credentials() -> Result<()> {
    for (base, expected) in [
        (
            "https://example.com/v1/",
            "https://example.com/v1/chat/completions",
        ),
        (
            "http://localhost:8080/v1/chat/completions",
            "http://localhost:8080/v1/chat/completions",
        ),
    ] {
        let config = VisionConfig {
            base_url: base.into(),
            ..Default::default()
        };
        assert_eq!(config.endpoint()?.as_str(), expected);
    }
    for base in [
        "http://example.com",
        "https://user:pass@example.com",
        "https://example.com?key=secret",
        "file:///tmp/api",
    ] {
        assert!(VisionConfig {
            base_url: base.into(),
            ..Default::default()
        }
        .endpoint()
        .is_err());
    }
    assert!(VisionClient::new(VisionConfig {
        api_key: "invalid\nkey".into(),
        ..Default::default()
    })
    .is_err());
    Ok(())
}
