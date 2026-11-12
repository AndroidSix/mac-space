#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <LocalAuthentication/LocalAuthentication.h>

int macspace_thermal_state(void) {
    return (int)[NSProcessInfo processInfo].thermalState;
}

int macspace_low_power_mode(void) {
    return [NSProcessInfo processInfo].lowPowerModeEnabled ? 1 : 0;
}

int macspace_system_is_chinese(void) {
    return [[[NSLocale preferredLanguages] firstObject] hasPrefix:@"zh"] ? 1 : 0;
}

static NSDictionary *token_query(void) {
    return @{(__bridge id)kSecClass: (__bridge id)kSecClassGenericPassword,
             (__bridge id)kSecAttrService: @"local.macspace.desktop.github",
             (__bridge id)kSecAttrAccount: @"release-check"};
}

char *macspace_github_token_read(void) {
    NSMutableDictionary *query = [token_query() mutableCopy];
    query[(__bridge id)kSecReturnData] = @YES;
    LAContext *context = [[LAContext alloc] init];
    context.interactionNotAllowed = YES;
    query[(__bridge id)kSecUseAuthenticationContext] = context;
    CFTypeRef result = NULL;
    if (SecItemCopyMatching((__bridge CFDictionaryRef)query, &result) != errSecSuccess) return NULL;
    NSData *data = CFBridgingRelease(result);
    NSString *token = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    return token ? strdup(token.UTF8String) : NULL;
}

int macspace_github_token_write(const char *token) {
    NSDictionary *query = token_query();
    if (!token[0]) {
        OSStatus status = SecItemDelete((__bridge CFDictionaryRef)query);
        return status == errSecItemNotFound ? 0 : (int)status;
    }
    NSData *data = [[NSString stringWithUTF8String:token] dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *attributes = @{(__bridge id)kSecValueData: data};
    OSStatus status = SecItemUpdate((__bridge CFDictionaryRef)query, (__bridge CFDictionaryRef)attributes);
    if (status == errSecItemNotFound) {
        NSMutableDictionary *item = [query mutableCopy];
        [item addEntriesFromDictionary:attributes];
        item[(__bridge id)kSecAttrAccessible] = (__bridge id)kSecAttrAccessibleWhenUnlockedThisDeviceOnly;
        status = SecItemAdd((__bridge CFDictionaryRef)item, NULL);
    }
    return (int)status;
}

int systembalance_set_low_power(int low) {
    if (low != 0 && low != 1) return -1;
    __block int result = -1;
    void (^change)(void) = ^{
        NSString *source = [NSString stringWithFormat:
            @"do shell script \"/usr/bin/pmset -a lowpowermode %d\" with administrator privileges", low];
        NSDictionary *error = nil;
        NSAppleScript *script = [[NSAppleScript alloc] initWithSource:source];
        [script executeAndReturnError:&error];
        result = error ? [error[NSAppleScriptErrorNumber] intValue] : 0;
    };
    if ([NSThread isMainThread]) change();
    else dispatch_sync(dispatch_get_main_queue(), change);
    return result;
}
