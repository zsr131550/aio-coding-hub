//! Usage: Descriptor-driven gateway plugin mutation enforcement.

use super::context::{GatewayHookResult, GatewayPluginHookName};
use super::permissions::GatewayPluginError;
use super::registry::HookDescriptor;

pub(crate) fn enforce_descriptor_permissions(
    descriptor: HookDescriptor,
    permissions: &[String],
    result: &GatewayHookResult,
) -> Result<(), GatewayPluginError> {
    if result.request_body.is_some() {
        require_mutation_field(descriptor, "requestBody", "request body mutation")?;
        require_permission(permissions, "request.body.write")?;
    }
    if result.response_body.is_some() {
        require_mutation_field(descriptor, "responseBody", "response body mutation")?;
        require_permission(permissions, "response.body.write")?;
    }
    if result.stream_chunk.is_some() {
        require_mutation_field(descriptor, "streamChunk", "stream chunk mutation")?;
        require_permission(permissions, "stream.modify")?;
    }
    if result.log_message.is_some() {
        require_mutation_field(descriptor, "logMessage", "log mutation")?;
        require_permission(permissions, "log.redact")?;
    }
    if !result.headers.is_empty() {
        require_header_mutation(descriptor, permissions)?;
    }
    Ok(())
}

fn require_mutation_field(
    descriptor: HookDescriptor,
    field: &'static str,
    operation: &'static str,
) -> Result<(), GatewayPluginError> {
    if descriptor.allows_mutation_field(field) {
        Ok(())
    } else {
        Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            format!(
                "{operation} is not allowed in {}",
                descriptor.hook_name.as_str()
            ),
        ))
    }
}

fn require_header_mutation(
    descriptor: HookDescriptor,
    permissions: &[String],
) -> Result<(), GatewayPluginError> {
    if !descriptor.allows_mutation_field("headers") {
        if descriptor.hook_name == GatewayPluginHookName::ResponseChunk {
            // Pre-descriptor enforcement accepted response header writes on stream chunks
            // even though stream outputs do not apply header patches.
            return require_permission(permissions, "response.header.write");
        }
        return Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            format!(
                "headers cannot be mutated in {}",
                descriptor.hook_name.as_str()
            ),
        ));
    }
    debug_assert!(
        descriptor.hook_name.is_request_hook() || descriptor.hook_name.is_response_hook()
    );

    let Some(permission) = header_write_permission(descriptor) else {
        return Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            format!(
                "headers cannot be mutated in {}",
                descriptor.hook_name.as_str()
            ),
        ));
    };

    require_permission(permissions, permission)
}

fn header_write_permission(descriptor: HookDescriptor) -> Option<&'static str> {
    if descriptor.allows_write_permission("request.header.write") {
        Some("request.header.write")
    } else if descriptor.allows_write_permission("response.header.write") {
        Some("response.header.write")
    } else {
        None
    }
}

fn require_permission(
    permissions: &[String],
    permission: &'static str,
) -> Result<(), GatewayPluginError> {
    if permissions.iter().any(|item| item == permission) {
        Ok(())
    } else {
        Err(GatewayPluginError::new(
            "PLUGIN_PERMISSION_DENIED",
            format!("missing plugin permission: {permission}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::plugins::context::GatewayPluginHookName;
    use crate::gateway::plugins::registry::HookRegistry;

    #[test]
    fn request_body_mutation_requires_descriptor_permission() {
        let descriptor = HookRegistry::new()
            .descriptor(GatewayPluginHookName::RequestBeforeSend)
            .expect("request before send descriptor should resolve");
        let result = GatewayHookResult {
            request_body: Some("changed".to_string()),
            ..GatewayHookResult::continue_unchanged()
        };

        let err = enforce_descriptor_permissions(descriptor, &[], &result)
            .expect_err("request body write should require permission");
        assert_eq!(err.code_for_logging(), "PLUGIN_PERMISSION_DENIED");

        enforce_descriptor_permissions(descriptor, &["request.body.write".to_string()], &result)
            .expect("request body write permission should allow mutation");
    }

    #[test]
    fn stream_chunk_header_mutation_preserves_legacy_permission_behavior() {
        let descriptor = HookRegistry::new()
            .descriptor(GatewayPluginHookName::ResponseChunk)
            .expect("response chunk descriptor should resolve");
        let mut result = GatewayHookResult::continue_unchanged();
        result
            .headers
            .insert("x-plugin".to_string(), "1".to_string());

        let err = enforce_descriptor_permissions(descriptor, &[], &result)
            .expect_err("legacy response header write should still require permission");
        assert_eq!(err.code_for_logging(), "PLUGIN_PERMISSION_DENIED");
        assert!(err.to_string().contains("response.header.write"));

        enforce_descriptor_permissions(descriptor, &["response.header.write".to_string()], &result)
            .expect("legacy response header write permission should allow stream hook result");
    }
}
