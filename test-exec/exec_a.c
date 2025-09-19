#include <stdlib.h>
#include <unistd.h>

int main() {
  system("ps -ef | tail");
  sleep(2);
  return 0;
}
