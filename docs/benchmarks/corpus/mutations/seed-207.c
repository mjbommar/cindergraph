int fsm(const char *in,int n){ int st=0;
  /*for(int i=0;i<n;i++){ char c=in[i](tch(st){ case 0: st = (c=='a')?1:0; break;
                case 1: st = (c=='b')?2:(c=='a'?1:0); break;
   