#include <iostream>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <pthread.h>

#define BUFF_LEN 20 //UDP payload
#define SERVER_PORT 8888
#define SERVER_IP “0.0.0.0”
using namespace std;
void* stop_server(void* arg){
    int* status=(int *)arg;
    char data[10];
    cout<<"press [Enter] to stop Server"<<endl;
    cin>>data;
    *status=0;
    return 0;
}
void* server_fun(void* arg){
    int serv_sock = socket(AF_INET, SOCK_DGRAM, 0);
    if (serv_sock<0){
        cout<<"socket fail"<<endl;
        return (void *)-1;
    }


    char buf[BUFF_LEN];
    socklen_t len;
    struct sockaddr_in server_info,clent_info;

    pthread_t num;

    server_info.sin_family = AF_INET;
    server_info.sin_addr.s_addr = htonl(INADDR_ANY); //INADDR_ANY  local address
    server_info.sin_port = htons(SERVER_PORT);    //server port predefine

    int ret = bind(serv_sock, (struct sockaddr*)&server_info, sizeof(server_info));
    if (ret<0) {
        cout << "socket fail" << endl;
        return (void *)-1;
    }
    int flag=1;
    pthread_create(&num,nullptr,stop_server,(void *)&flag);
    while (flag){
        len=sizeof(clent_info);
        int count = recvfrom(serv_sock, buf, BUFF_LEN, 0, (struct sockaddr*)&clent_info,
                         &len);
        if (count==-1){
            cout<<"recive fail"<<endl;
            return (void *)-1;
        }
        cout<<"server:"<<buf<<endl;
        sprintf(buf, "I recieved %d", count);
        sendto(serv_sock, buf, BUFF_LEN, 0, (struct sockaddr*)&clent_info, len);
    }
    close(serv_sock);
    return 0;
}

void* client_fun(void* arg){
    int client_fd;
    struct sockaddr_in ser_addr;

    client_fd = socket(AF_INET, SOCK_DGRAM, 0);
    if(client_fd < 0) {
        printf("create socket fail!\n");
        return (void *)-1;
    }
    memset(&ser_addr, 0, sizeof(ser_addr));
    ser_addr.sin_family = AF_INET;
    //ser_addr.sin_addr.s_addr = inet_addr(SERVER_IP);
    ser_addr.sin_addr.s_addr = htonl(INADDR_ANY);
    ser_addr.sin_port = htons(SERVER_PORT);

    socklen_t len;
    struct sockaddr_in src;
    int flag=1;
    pthread_t num;
    pthread_create(&num,nullptr,stop_server,(void *)&flag);
    while(flag) {
        char buf[BUFF_LEN] = "TEST UDP\n";
        len = sizeof(ser_addr);
        sendto(client_fd, buf, BUFF_LEN, 0, (struct sockaddr *)&ser_addr, len);
        memset(buf, 0, BUFF_LEN);
        recvfrom(client_fd, buf, BUFF_LEN, 0, (struct sockaddr *) &src, &len);
        cout<<"client:"<<buf<<endl;
        sleep(1);
    }

    close(client_fd);
    return 0;
}

int main() {
    pthread_t num1,num2;
    pthread_create(&num1,nullptr,server_fun,NULL);
    sleep(1);

    pthread_create(&num2,nullptr,client_fun,NULL);
    pthread_exit(NULL);
    return 0;
}
