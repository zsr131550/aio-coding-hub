import { describe, expect, it } from "vitest";
import {
  formatHostPort,
  parseCustomListenAddress,
  validateCx2ccFallbackModel,
  validateCx2ccOptionalField,
  validateGatewayCustomListenAddress,
  validateSettingsSetInput,
  validateUpstreamProxyFields,
  validateWslCustomHostAddress,
} from "../settingsValidation";

describe("services/settings/settingsValidation", () => {
  it("accepts backend-aligned numeric boundary values", () => {
    expect(
      validateSettingsSetInput({
        preferredPort: 1024,
        logRetentionDays: 3650,
        providerCooldownSeconds: 0,
        providerBaseUrlPingCacheTtlSeconds: 1,
        upstreamFirstByteTimeoutSeconds: 3600,
        upstreamStreamIdleTimeoutSeconds: 0,
        upstreamRequestTimeoutNonStreamingSeconds: 86400,
        failoverMaxAttemptsPerProvider: 20,
        failoverMaxProvidersToTry: 5,
        circuitBreakerFailureThreshold: 50,
        circuitBreakerOpenDurationMinutes: 1440,
      })
    ).toBeNull();

    expect(validateSettingsSetInput({ upstreamStreamIdleTimeoutSeconds: 60 })).toBeNull();
  });

  it("rejects numeric settings outside backend bounds before IPC", () => {
    expect(validateSettingsSetInput({ preferredPort: 1023 })).toContain("首选端口必须 >= 1024");
    expect(validateSettingsSetInput({ logRetentionDays: 3651 })).toContain(
      "日志保留天数必须 <= 3650"
    );
    expect(validateSettingsSetInput({ providerCooldownSeconds: 3601 })).toContain(
      "Provider 冷却时间必须 <= 3600"
    );
    expect(validateSettingsSetInput({ providerBaseUrlPingCacheTtlSeconds: 0 })).toContain(
      "Provider Base URL 探测缓存 TTL必须 >= 1"
    );
    expect(validateSettingsSetInput({ upstreamFirstByteTimeoutSeconds: 3601 })).toContain(
      "首字节超时必须 <= 3600"
    );
    expect(
      validateSettingsSetInput({ upstreamRequestTimeoutNonStreamingSeconds: 86401 })
    ).toContain("非流式请求超时必须 <= 86400");
    expect(validateSettingsSetInput({ circuitBreakerFailureThreshold: 0 })).toContain(
      "熔断失败阈值必须 >= 1"
    );
    expect(validateSettingsSetInput({ circuitBreakerOpenDurationMinutes: 1441 })).toContain(
      "熔断打开时长必须 <= 1440"
    );
  });

  it("rejects fractional values and stream idle timeout values in the forbidden gap", () => {
    expect(validateSettingsSetInput({ preferredPort: 37123.5 })).toContain("首选端口必须是整数");
    expect(validateSettingsSetInput({ upstreamStreamIdleTimeoutSeconds: 30 })).toContain(
      "流式空闲超时必须为 0"
    );
    expect(validateSettingsSetInput({ upstreamStreamIdleTimeoutSeconds: 3601 })).toContain(
      "流式空闲超时必须 <= 3600"
    );
  });

  it("rejects failover product overflow when both dimensions are present", () => {
    expect(
      validateSettingsSetInput({
        failoverMaxAttemptsPerProvider: 20,
        failoverMaxProvidersToTry: 6,
      })
    ).toContain("Failover 总尝试次数必须 <= 100");
  });

  it("parses and formats custom gateway listen addresses", () => {
    expect(parseCustomListenAddress("")).toEqual({ host: "0.0.0.0", port: null });
    expect(parseCustomListenAddress("127.0.0.1:37123")).toEqual({
      host: "127.0.0.1",
      port: 37123,
    });
    expect(parseCustomListenAddress("[::1]:37123")).toEqual({ host: "::1", port: 37123 });
    expect(parseCustomListenAddress("https://127.0.0.1:37123")).toBeNull();
    expect(parseCustomListenAddress("127.0.0.1:80")).toBeNull();

    expect(formatHostPort("127.0.0.1", 37123)).toBe("127.0.0.1:37123");
    expect(formatHostPort("::1", 37123)).toBe("[::1]:37123");
    expect(validateGatewayCustomListenAddress("127.0.0.1:abc")).toContain("自定义地址仅支持");
    expect(validateGatewayCustomListenAddress("127.0.0.1:80")).toContain("端口必须 >= 1024");
  });

  it("validates WSL custom host values without accepting URLs or ports", () => {
    expect(validateWslCustomHostAddress("host.docker.internal")).toBeNull();
    expect(validateWslCustomHostAddress("[::1]")).toBeNull();
    expect(validateWslCustomHostAddress("::1")).toBeNull();
    expect(validateWslCustomHostAddress("http://localhost")).toContain("不要包含协议或路径");
    expect(validateWslCustomHostAddress("[::1")).toContain("缺少右方括号");
    expect(validateWslCustomHostAddress("[::1]:37123")).toContain("不要包含端口");
    expect(validateWslCustomHostAddress("127.0.0.1:37123")).toContain("不支持端口");
  });

  it("validates update URLs, proxy credentials, and CX2CC text fields", () => {
    expect(
      validateSettingsSetInput({
        updateReleasesUrl: "ftp://example.com/releases.json",
      })
    ).toContain("更新地址仅支持 http 或 https");
    expect(validateSettingsSetInput({ updateReleasesUrl: "https://u:p@example.com" })).toContain(
      "更新地址不能包含用户名或密码"
    );
    expect(
      validateSettingsSetInput({ updateReleasesUrl: "https://example.com/releases" })
    ).toBeNull();

    expect(validateUpstreamProxyFields({ enabled: true, url: "" })).toContain("代理地址不能为空");
    expect(
      validateUpstreamProxyFields({ url: "not a url", validateUrlWhenPresent: true })
    ).toContain("代理地址不是有效 URL");
    expect(
      validateUpstreamProxyFields({ url: "ftp://example.com", validateUrlWhenPresent: true })
    ).toContain("代理地址协议仅支持");
    expect(
      validateUpstreamProxyFields({
        url: "https://user:pass@example.com",
        username: "user",
        validateUrlWhenPresent: true,
      })
    ).toContain("代理认证信息不要同时写在 URL");
    expect(
      validateUpstreamProxyFields({ url: "https://example.com", username: "user" })
    ).toBeNull();
    expect(
      validateUpstreamProxyFields({
        url: "https://example.com",
        passwordUpdate: { mode: "replace", value: "secret" },
      })
    ).toContain("填写代理密码时也需要填写用户名");

    expect(validateCx2ccFallbackModel("模型", " claude-3 ")).toBeNull();
    expect(validateCx2ccFallbackModel("模型", "")).toContain("模型不能为空");
    expect(validateCx2ccFallbackModel("模型", "bad\u0000name")).toContain("模型不能包含控制字符");
    expect(validateCx2ccOptionalField("推理强度", "")).toBeNull();
    expect(validateCx2ccOptionalField("推理强度", "x".repeat(65))).toContain("推理强度必须 <=");
  });

  it("runs composite settings validation only for enabled custom modes", () => {
    expect(
      validateSettingsSetInput({
        gatewayListenMode: "custom",
        gatewayCustomListenAddress: "127.0.0.1:80",
      })
    ).toContain("端口必须 >= 1024");
    expect(
      validateSettingsSetInput({
        gatewayListenMode: "localhost",
        gatewayCustomListenAddress: "127.0.0.1:80",
      })
    ).toBeNull();
    expect(
      validateSettingsSetInput({
        wslHostAddressMode: "custom",
        wslCustomHostAddress: "127.0.0.1:37123",
      })
    ).toContain("不支持端口");
    expect(
      validateSettingsSetInput({
        wslHostAddressMode: "auto",
        wslCustomHostAddress: "127.0.0.1:37123",
      })
    ).toBeNull();
  });
});
