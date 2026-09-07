#include <copyfile.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/clonefile.h>
#include <unistd.h>

int main(int argc,char **argv) {
    if (argc<3) return 2;
    if (!strcmp(argv[1],"clone")) return clonefile(argv[2],argv[3],0)!=0;
    int fd=open(argv[2],O_WRONLY|O_CREAT|O_EXCL,0600);
    if (fd<0) return 3;
    char data[65536]; memset(data,'x',sizeof(data));
    for(int i=0;i<512;i++) if(write(fd,data,sizeof(data))!=sizeof(data)) return 4;
    if (!strcmp(argv[1],"sync") && fsync(fd)) return 5;
    return close(fd)!=0;
}
