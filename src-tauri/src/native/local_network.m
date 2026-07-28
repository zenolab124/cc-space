// 本地网络权限探测（走 Network.framework）。
//
// 为什么不能用 BSD socket 测：macOS 的本地网络隐私只管辖 Network.framework 这条路径，
// BSD socket 不受约束。实测同一进程树内 curl / node（BSD socket）畅通，而 CLI 子进程
// （Bun，走 Network.framework）连同一个局域网地址零包发出、报 FailedToOpenSocket。
// 用 BSD socket 检测必然假阳性——它测的是一条永远不会出问题的路。
//
// 返回值：0=可达（权限正常） 1=静默失败（疑似权限拒绝） -1=参数/内部错误
// 判据：拿到 ready 固然是通；拿到 ECONNREFUSED/ECONNRESET 同样算通——收到对端拒绝
// 说明包已经出网，权限没挡。只有超时/静默无响应才是权限拒绝的特征。

#include <Network/Network.h>
#include <dispatch/dispatch.h>
#include <errno.h>

int monet_nw_probe(const char *host, const char *port, int timeout_ms) {
    if (!host || !port || timeout_ms <= 0) return -1;

    nw_endpoint_t ep = nw_endpoint_create_host(host, port);
    if (!ep) return -1;

    nw_parameters_t params = nw_parameters_create_secure_tcp(
        NW_PARAMETERS_DISABLE_PROTOCOL,      // 不要 TLS，只验证 TCP 可达
        NW_PARAMETERS_DEFAULT_CONFIGURATION);
    if (!params) return -1;

    nw_connection_t conn = nw_connection_create(ep, params);
    if (!conn) return -1;

    dispatch_queue_t q = dispatch_queue_create("io.github.zenolab124.monet.nwprobe",
                                               DISPATCH_QUEUE_SERIAL);
    nw_connection_set_queue(conn, q);

    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    __block int result = 1;

    nw_connection_set_state_changed_handler(conn, ^(nw_connection_state_t state,
                                                    nw_error_t error) {
        if (state == nw_connection_state_ready) {
            result = 0;
            dispatch_semaphore_signal(sem);
        } else if (state == nw_connection_state_failed ||
                   state == nw_connection_state_cancelled) {
            int code = error ? nw_error_get_error_code(error) : 0;
            result = (code == ECONNREFUSED || code == ECONNRESET) ? 0 : 1;
            dispatch_semaphore_signal(sem);
        }
    });

    nw_connection_start(conn);

    dispatch_time_t deadline =
        dispatch_time(DISPATCH_TIME_NOW, (int64_t)timeout_ms * NSEC_PER_MSEC);
    if (dispatch_semaphore_wait(sem, deadline) != 0) {
        result = 1;  // 超时：受管路径静默吞包，正是权限拒绝的表现
    }

    nw_connection_cancel(conn);
    return result;
}
