// macOS 13+ 后台项目注册桥接。
//
// SMAppService 是 launchd 手写 LaunchAgent / LoginItem 的系统管理入口。
// 这里用 C ABI 暴露给 Rust，保持主工程现有的 native bridge 结构，不引入
// 额外的 Swift runtime 或需要付费开发者账号的能力。

#import <Foundation/Foundation.h>
#import <ServiceManagement/ServiceManagement.h>

#include <stddef.h>
#include <string.h>

enum {
    MONET_SM_LOGIN_ITEM = 0,
    MONET_SM_LAUNCH_AGENT = 1,
    MONET_SM_UNAVAILABLE = 4,
};

static SMAppService *monet_service(int kind, const char *value) API_AVAILABLE(macos(13.0)) {
    if (!value) return nil;
    NSString *name = [NSString stringWithUTF8String:value];
    if (!name) return nil;

    if (kind == MONET_SM_LOGIN_ITEM) {
        return [SMAppService loginItemServiceWithIdentifier:name];
    }
    if (kind == MONET_SM_LAUNCH_AGENT) {
        return [SMAppService agentServiceWithPlistName:name];
    }
    return nil;
}

static void monet_copy_error(NSError *error, char *buffer, size_t capacity) {
    if (!buffer || capacity == 0) return;
    buffer[0] = '\0';

    NSString *message = error
        ? [NSString stringWithFormat:@"%@ (code %ld)", error.localizedDescription, (long)error.code]
        : @"ServiceManagement returned an unknown error";
    const char *utf8 = message.UTF8String;
    if (!utf8) return;
    strncpy(buffer, utf8, capacity - 1);
    buffer[capacity - 1] = '\0';
}

int monet_sm_available(void) {
    if (@available(macOS 13.0, *)) return 1;
    return 0;
}

int monet_sm_status(int kind, const char *value) {
    if (@available(macOS 13.0, *)) {
        SMAppService *service = monet_service(kind, value);
        return service ? (int)service.status : (int)SMAppServiceStatusNotFound;
    }
    return MONET_SM_UNAVAILABLE;
}

int monet_sm_register(int kind, const char *value, char *error, size_t error_capacity) {
    if (@available(macOS 13.0, *)) {
        SMAppService *service = monet_service(kind, value);
        if (!service) {
            monet_copy_error(nil, error, error_capacity);
            return 1;
        }
        NSError *registration_error = nil;
        if ([service registerAndReturnError:&registration_error]) return 0;
        monet_copy_error(registration_error, error, error_capacity);
        return 1;
    }
    monet_copy_error(nil, error, error_capacity);
    return 1;
}

int monet_sm_unregister(int kind, const char *value, char *error, size_t error_capacity) {
    if (@available(macOS 13.0, *)) {
        SMAppService *service = monet_service(kind, value);
        if (!service) {
            monet_copy_error(nil, error, error_capacity);
            return 1;
        }
        NSError *unregistration_error = nil;
        if ([service unregisterAndReturnError:&unregistration_error]) return 0;
        monet_copy_error(unregistration_error, error, error_capacity);
        return 1;
    }
    monet_copy_error(nil, error, error_capacity);
    return 1;
}

void monet_sm_open_login_items(void) {
    if (@available(macOS 13.0, *)) {
        [SMAppService openSystemSettingsLoginItems];
    }
}
