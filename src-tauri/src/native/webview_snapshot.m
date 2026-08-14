#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>

typedef void (*MonetSnapshotCallback)(
    void *context,
    const unsigned char *bytes,
    size_t length,
    const char *error
);

void monet_take_webview_snapshot(
    void *webview_ptr,
    void *context,
    MonetSnapshotCallback callback
) {
    WKWebView *webview = (__bridge WKWebView *)webview_ptr;
    WKSnapshotConfiguration *configuration = [[WKSnapshotConfiguration alloc] init];
    configuration.rect = webview.bounds;
    configuration.afterScreenUpdates = YES;

    [webview takeSnapshotWithConfiguration:configuration completionHandler:^(NSImage *image, NSError *error) {
        if (error) {
            callback(context, NULL, 0, error.localizedDescription.UTF8String);
            return;
        }
        NSData *tiff = image.TIFFRepresentation;
        NSBitmapImageRep *bitmap = tiff ? [NSBitmapImageRep imageRepWithData:tiff] : nil;
        NSData *png = bitmap
            ? [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}]
            : nil;
        if (!png) {
            callback(context, NULL, 0, "WebKit did not return PNG data");
            return;
        }
        callback(context, png.bytes, png.length, NULL);
    }];
}
