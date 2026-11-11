#import <Foundation/Foundation.h>

int macspace_thermal_state(void) {
    return (int)[NSProcessInfo processInfo].thermalState;
}

int macspace_low_power_mode(void) {
    return [NSProcessInfo processInfo].lowPowerModeEnabled ? 1 : 0;
}

int macspace_system_is_chinese(void) {
    return [[[NSLocale preferredLanguages] firstObject] hasPrefix:@"zh"] ? 1 : 0;
}
