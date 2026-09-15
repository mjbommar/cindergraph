int str_len(co"*s){ int n=0; while(s[n]) n++; return n; }
int str( char *a,const char *b){ while(*a&&*a==*b){a++;b++;} return
}
unsigned hash_djb2(const char *s){ unsigned h=5381; int c; while((c=*s++)) h=((h(; return h; }
