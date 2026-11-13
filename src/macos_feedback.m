#import <AppKit/AppKit.h>

static NSWindow *flashWindow;

void qs_hide_flash(void) {
    [flashWindow orderOut:nil];
    flashWindow = nil;
}

void qs_show_flash(uint32_t displayID) {
    qs_hide_flash();
    for (NSScreen *screen in NSScreen.screens) {
        if ([screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue] != displayID) continue;
        flashWindow = [[NSWindow alloc] initWithContentRect:screen.frame styleMask:NSWindowStyleMaskBorderless
            backing:NSBackingStoreBuffered defer:NO];
        flashWindow.backgroundColor = NSColor.whiteColor;
        flashWindow.alphaValue = 0.30;
        flashWindow.animationBehavior = NSWindowAnimationBehaviorNone;
        flashWindow.ignoresMouseEvents = YES;
        flashWindow.hasShadow = NO;
        flashWindow.sharingType = NSWindowSharingNone;
        flashWindow.level = NSScreenSaverWindowLevel;
        flashWindow.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
        [flashWindow display];
        [flashWindow orderFrontRegardless];
        break;
    }
}
