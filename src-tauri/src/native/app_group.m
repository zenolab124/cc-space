#import <Foundation/Foundation.h>
#import <mach-o/dyld.h>

#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

static NSString *MonetAppGroupIdentifier(void) {
    const char *override = getenv("MONET_APP_GROUP_ID");
    if (override != NULL && override[0] != '\0') {
        return [NSString stringWithUTF8String:override];
    }

    id value = [[NSBundle mainBundle] objectForInfoDictionaryKey:@"MonetAppGroupIdentifier"];
    if (value == nil) {
        // widget-updater 是 Monet.app/Contents/MacOS 下的第二个可执行文件，
        // NSBundle.mainBundle 不保证把它识别成外层 app，因此从真实 executable 反查。
        uint32_t length = 0;
        _NSGetExecutablePath(NULL, &length);
        char *buffer = malloc(length);
        if (buffer != NULL && _NSGetExecutablePath(buffer, &length) == 0) {
            NSURL *executable = [[NSURL fileURLWithPath:[NSString stringWithUTF8String:buffer]]
                URLByResolvingSymlinksInPath];
            NSURL *bundleURL = [[[executable URLByDeletingLastPathComponent]
                URLByDeletingLastPathComponent] URLByDeletingLastPathComponent];
            value = [[NSBundle bundleWithURL:bundleURL]
                objectForInfoDictionaryKey:@"MonetAppGroupIdentifier"];
        }
        free(buffer);
    }
    if (![value isKindOfClass:[NSString class]]) {
        return nil;
    }
    NSString *identifier = (NSString *)value;
    return identifier.length > 0 ? identifier : nil;
}

bool monet_app_group_is_configured(void) {
    @autoreleasepool {
        return MonetAppGroupIdentifier() != nil;
    }
}

char *monet_app_group_container_path(void) {
    @autoreleasepool {
        NSString *identifier = MonetAppGroupIdentifier();
        if (identifier == nil) {
            return NULL;
        }

        NSURL *container = [[NSFileManager defaultManager]
            containerURLForSecurityApplicationGroupIdentifier:identifier];
        if (container == nil) {
            return NULL;
        }
        return strdup(container.fileSystemRepresentation);
    }
}

void monet_app_group_free_path(char *path) {
    free(path);
}
