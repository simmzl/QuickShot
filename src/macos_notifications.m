#import <AppKit/AppKit.h>
#import <UserNotifications/UserNotifications.h>

static NSString *const QSSavedCapture = @"QuickShot.SavedCapture";
static NSString *const QSReveal = @"QuickShot.Reveal";

@interface QSNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@end

@implementation QSNotificationDelegate
- (void)userNotificationCenter:(UNUserNotificationCenter *)center
       willPresentNotification:(UNNotification *)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions))completion {
    completion(UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionList);
}

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
 didReceiveNotificationResponse:(UNNotificationResponse *)response
         withCompletionHandler:(void (^)(void))completion {
    NSString *path = response.notification.request.content.userInfo[@"path"];
    NSString *action = response.actionIdentifier;
    if (!path || !([action isEqual:UNNotificationDefaultActionIdentifier] || [action isEqual:QSReveal])) {
        completion();
        return;
    }
    dispatch_async(dispatch_get_main_queue(), ^{
        BOOL selected = [NSWorkspace.sharedWorkspace selectFile:path
                                     inFileViewerRootedAtPath:path.stringByDeletingLastPathComponent];
        NSLog(@"QuickShot Finder reveal %@", selected ? @"succeeded" : @"failed");
        completion();
    });
}
@end

static void postCapture(NSDictionary *capture, dispatch_group_t pending) {
    UNMutableNotificationContent *content = [UNMutableNotificationContent new];
    content.title = [capture[@"path"] isKindOfClass:NSString.class] ? @"Screenshot saved" : @"Screenshot copied";
    content.body = capture[@"body"];
    if ([capture[@"path"] isKindOfClass:NSString.class]) {
        content.categoryIdentifier = QSSavedCapture;
        content.userInfo = @{@"path": capture[@"path"]};
    }
    UNNotificationRequest *request = [UNNotificationRequest requestWithIdentifier:NSUUID.UUID.UUIDString
        content:content trigger:nil];
    UNUserNotificationCenter *center = UNUserNotificationCenter.currentNotificationCenter;
    dispatch_group_enter(pending);
    [center requestAuthorizationWithOptions:UNAuthorizationOptionAlert completionHandler:^(BOOL granted, NSError *error) {
        if (!granted || error) {
            NSLog(@"QuickShot notification authorization: granted=%d error=%@", granted, error);
            dispatch_group_leave(pending);
            return;
        }
        [center addNotificationRequest:request withCompletionHandler:^(NSError *deliveryError) {
            if (deliveryError) NSLog(@"QuickShot notification: %@", deliveryError);
            dispatch_group_leave(pending);
        }];
    }];
}

// The capture app owns stdin. On quit, finish posting accepted captures before exiting.
// Launch Services can reopen this bundle directly when a saved notification is clicked.
static void listenForCaptures(void) {
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        char *line = NULL;
        size_t capacity = 0;
        ssize_t length;
        dispatch_group_t pending = dispatch_group_create();
        while ((length = getline(&line, &capacity, stdin)) >= 0) {
            @autoreleasepool {
                NSData *data = [NSData dataWithBytes:line length:(NSUInteger)length];
                NSDictionary *capture = [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
                if ([capture isKindOfClass:NSDictionary.class] && [capture[@"body"] isKindOfClass:NSString.class]) {
                    postCapture(capture, pending);
                }
            }
        }
        free(line);
        dispatch_group_notify(pending, dispatch_get_main_queue(), ^{ [NSApp terminate:nil]; });
    });
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        static QSNotificationDelegate *delegate;
        delegate = [QSNotificationDelegate new];
        UNUserNotificationCenter *center = UNUserNotificationCenter.currentNotificationCenter;
        center.delegate = delegate;
        UNNotificationAction *reveal = [UNNotificationAction actionWithIdentifier:QSReveal
            title:@"点击查看" options:UNNotificationActionOptionForeground];
        UNNotificationCategory *category = [UNNotificationCategory categoryWithIdentifier:QSSavedCapture
            actions:@[reveal] intentIdentifiers:@[] options:UNNotificationCategoryOptionNone];
        [center setNotificationCategories:[NSSet setWithObject:category]];
        if (argc == 2 && !strcmp(argv[1], "--listen")) {
            listenForCaptures();
        } else {
            // Cold notification activation. Allow the OS to reclaim the idle helper.
            NSProcessInfo.processInfo.automaticTerminationSupportEnabled = YES;
        }
        [NSApp run];
    }
    return 0;
}
