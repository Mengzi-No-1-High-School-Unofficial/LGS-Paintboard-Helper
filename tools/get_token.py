import pandas as pd
import json
from curl_cffi import requests
from pathlib import Path
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed

USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0";
CF_CLEARANCE = "vC0hw8NAzJI3DEP85XM0P9f64lEOPlSh1GAZZqxLB6Q-1761403850-1.2.1.1-oFYOrS9bcAxx6vttbK6WZ_X9yg9ohGic4eDX2a6.XI3KoMUAHy7urowJwRo8IQiSSB3HREQcu7gVoAgFIE5CQqqlZ56zmda4mNec_FSbtX45votkRhV7eBW3mERDfhC_4Nb2U7vBYFSKdM1bDztplNtl19x0KSGlBEejpSrs28o_4hY8.hl2Iskr2Jhqx3pPEIlRFXwbZfN2K7rpFAy5hPpvjnryphjxpl3klbs9gCI"

def login_lgs(uid: int, token: str) -> str:
    url = "https://www.luogu.me/user/login"
    payload = {
        "token": token
    }
    
    try:
        response = requests.post(url, data=payload, headers={"User-Agent": USER_AGENT}, cookies={"cf_clearance": CF_CLEARANCE}, impersonate='chrome110')
        if response.status_code == 200:
            new_token = response.cookies.get("token")
            
            if new_token:
                return new_token
            else:
                raise Exception("Token not found in cookies")
        else:
            raise Exception(f"Login failed with status code: {response.status_code}")
    except Exception as e:
        print(f"Error during login for UID {uid}: {str(e)}")
        raise

def get_accesskey(uid: int, login_token: str) -> str:
    url = "https://www.luogu.me/paintboard/apply"
    cookies = {
        'token': login_token,
        'cf_clearance': CF_CLEARANCE
    }
    
    try:
        response = requests.post(url, cookies=cookies, headers={"User-Agent": USER_AGENT}, impersonate='chrome110')
        if response.status_code == 200:
            data = response.json()
            accesskey = data.get("token")
            
            if accesskey:
                return accesskey
            else:
                raise Exception(f"Accesskey not found in response: {response.text}")
        else:
            raise Exception(f"Failed to get accesskey with status code: {response.status_code}, response: {response.text}")
    except Exception as e:
        print(f"Error getting accesskey for UID {uid}: {str(e)}")
        raise

def get_token(uid: int, accesskey: str) -> str:
    url = "https://paintboard.luogu.me/api/auth/gettoken"
    try:
        response = requests.post(url, json={"access_key": accesskey, 'uid': uid})
        data = response.json()
        
        if data['data'].get("errorType", None):
            raise Exception(f"Error getting token: {data}")
        
        return data['data']['token']
    except Exception as e:
        print(f"Error getting token for UID {uid}: {str(e)}")
        raise

def process_row(index, row):
    """处理单行数据并返回结果"""
    print(f"Getting token for user: {row['UID']}")
    
    try:
        login_token = login_lgs(int(row['UID']), row['保存站 Token'])
        accesskey = get_accesskey(int(row['UID']), login_token)
        
        print(f"UID: {row['UID']}, Accesskey: {accesskey[0:8]}...")
        
        token = get_token(int(row['UID']), accesskey)
        
        print(f"UID: {row['UID']}, Token: {token[0:8]}...\n")
        
        # 返回处理结果
        return index, accesskey, token, '已获取'
        
    except Exception as e:
        print(f"Failed to process UID {row['UID']}: {str(e)}")
        return index, None, None, '获取失败'

def main():
    df = pd.read_csv("tokens.csv")
    
    print(f"Processing {len(df)} rows in tokens.csv")
    print("Starting token retrieval process...")
    
    # 先统计需要处理的行数
    rows_to_process = []
    for index in df.index:
        row = df.loc[index]
        # 检查是否已经有PaintKey，如果没有则加入处理列表
        if pd.isna(row['PaintKey']) or row['PaintKey'] == '':
            rows_to_process.append((index, row))
    
    print(f"Found {len(rows_to_process)} rows without PaintKey to process")
    
    # 使用线程池处理没有PaintKey的行
    with ThreadPoolExecutor(max_workers=3) as executor:  # 限制并发数为3，避免对服务器造成过大压力
        # 提交任务
        future_to_index = {executor.submit(process_row, index, row): index for index, row in rows_to_process}
        
        # 处理完成的任务
        for future in as_completed(future_to_index):
            index, accesskey, token, _status = future.result()
            
            # 更新DataFrame
            if accesskey is not None and token is not None:
                df.at[index, 'AccessKey'] = accesskey
                df.at[index, 'PaintKey'] = token
    
    # 显示处理后的表格
    print("\nUpdated table:")
    print(df.to_string())
    
    # 等待用户确认后保存
    input("\nPress Enter to save to tokens.csv...")
    
    # 保存最终结果
    df.to_csv("tokens.csv", index=False)
    print("Saved updated tokens.csv")

if __name__ == "__main__":
    main()