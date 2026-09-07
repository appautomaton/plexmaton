#include <libproc.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

static void record(const char *event) {
    const char *path = getenv("PLEXMATON_IO_SOCKET");
    if (!path) return;
    struct rusage_info_v4 r = {0};
    char name[256] = {0};
    proc_name(getpid(), name, sizeof(name));
    const char *package=getenv("CARGO_PKG_NAME");
    if (!package) package="";
    int rc = proc_pid_rusage(getpid(), RUSAGE_INFO_V4, (rusage_info_t *)&r);
    char data[1024];
    int n = snprintf(data,sizeof(data),"{\"event\":\"%s\",\"pid\":%d,\"ppid\":%d,\"pgid\":%d,\"name\":\"%s\",\"package\":\"%s\",\"rc\":%d,\"start\":%llu,\"disk_read\":%llu,\"disk_write\":%llu,\"logical_write\":%llu}\n",
        event,getpid(),getppid(),getpgrp(),name,package,rc,r.ri_proc_start_abstime,r.ri_diskio_bytesread,r.ri_diskio_byteswritten,r.ri_logical_writes);
    int fd=socket(AF_UNIX,SOCK_DGRAM,0);
    if (fd>=0) {
        struct sockaddr_un address={0}; address.sun_family=AF_UNIX;
        if (strlen(path)<sizeof(address.sun_path)) {
            strcpy(address.sun_path,path);
            if (n>0 && n<(int)sizeof(data))
                (void)sendto(fd,data,(size_t)n,0,(struct sockaddr *)&address,sizeof(address));
        }
        close(fd);
    }
}
__attribute__((constructor)) static void begin(void) { record("start"); }
__attribute__((destructor)) static void end(void) { record("end"); }
